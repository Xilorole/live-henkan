//! Incremental romaji to hiragana conversion for IME use.
//!
//! Wraps [`wana_kana`] (batch conversion) with a stateful interface that
//! processes one character at a time, tracking confirmed hiragana vs.
//! pending romaji.
//!
//! # Design rationale
//!
//! Rather than implementing a custom Trie-based state machine (bug-prone,
//! must handle all edge cases of romaji tables), we delegate to `wana_kana`
//! which is well-tested and handles all standard romaji patterns including
//! digraphs (sh, ch, ts), y-combos, double consonants (っ), and ん ambiguity.
//!
//! The incremental wrapper simply accumulates a romaji buffer, calls
//! `wana_kana::to_hiragana` on each keystroke, and diffs the result to
//! determine what has been newly confirmed. Performance is not a concern:
//! `wana_kana` converts ~1000 words/ms.
//!
//! # Example
//!
//! ```
//! use romaji::IncrementalRomaji;
//!
//! let mut conv = IncrementalRomaji::new();
//! let out = conv.feed('k');
//! assert_eq!(out.confirmed, "");
//! assert_eq!(out.pending, "k");
//!
//! let out = conv.feed('a');
//! assert_eq!(out.confirmed, "か");
//! assert_eq!(out.pending, "");
//! ```

use wana_kana::ConvertJapanese;

/// Result of feeding a single character to the converter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomajiOutput {
    /// Newly confirmed hiragana from this keystroke.
    pub confirmed: String,
    /// Remaining romaji that has not resolved to hiragana yet.
    pub pending: String,
}

/// Incremental romaji to hiragana converter for IME input.
///
/// Accumulates a romaji buffer internally and uses `wana_kana::to_hiragana`
/// on each keystroke. Compares the output with the previous state to determine
/// what has been newly confirmed.
#[derive(Debug, Default)]
pub struct IncrementalRomaji {
    /// Raw romaji buffer (only the trailing unresolved portion).
    buffer: String,
    /// Hiragana confirmed so far in the current composition.
    confirmed_so_far: String,
    /// When true, the pending 'n' in buffer was left by an "nn" sequence
    /// as a lookahead — it should form syllables like na→な, ni→に
    /// but should NOT be flushed as another ん on commit.
    nn_residual: bool,
}

impl IncrementalRomaji {
    /// Create a new converter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a single ASCII character and return confirmed hiragana + pending romaji.
    ///
    /// Non-ASCII characters are passed through as-is (immediately confirmed).
    pub fn feed(&mut self, ch: char) -> RomajiOutput {
        if !ch.is_ascii_alphabetic() {
            let flushed = self.flush_pending();
            let mut confirmed = flushed;
            confirmed.push(ch);
            return RomajiOutput {
                confirmed,
                pending: String::new(),
            };
        }

        self.buffer.push(ch);

        // Handle nn_residual: the leading 'n' was a lookahead from "nn".
        // If the new char is a vowel or 'y', the 'n' can form a syllable
        // (na→な, ni→に, nya→にゃ), so let wana_kana handle it normally.
        // If the new char is anything else (consonant), the lookahead 'n'
        // can't form a syllable — discard it and process the new char alone.
        if self.nn_residual {
            self.nn_residual = false;
            if !matches!(ch, 'a' | 'i' | 'u' | 'e' | 'o' | 'y') {
                // Discard the residual 'n', keep only the new char
                self.buffer = ch.to_string();
            }
        }

        // Handle 'n' ambiguity for incremental IME use.
        //
        // wana_kana v4 eagerly converts "n" → "ん" and "nn" → "んん",
        // which breaks incremental romaji input.
        //
        // "nn": confirm ん, keep second 'n' as residual lookahead
        // (so "nna" → "んな" and "nni" → "んに" work on next keystroke).

        if self.buffer.ends_with("nn") {
            // "nn" detected: confirm everything before + "ん"
            let before_nn = &self.buffer[..self.buffer.len() - 2];
            let mut confirmed = String::new();

            if !before_nn.is_empty() {
                let converted = before_nn.to_hiragana();
                let (hiragana_part, _romaji_tail) = split_trailing_romaji(&converted);
                confirmed.push_str(&hiragana_part);
            }
            confirmed.push('ん');
            self.confirmed_so_far.push_str(&confirmed);
            self.buffer = "n".to_string();
            self.nn_residual = true;

            return RomajiOutput {
                confirmed,
                pending: self.buffer.clone(),
            };
        }

        if self.buffer.ends_with('n') {
            // Trailing lone 'n': don't convert yet, keep pending.
            // Process everything before the 'n' normally.
            let before_n = &self.buffer[..self.buffer.len() - 1];
            if before_n.is_empty() {
                return RomajiOutput {
                    confirmed: String::new(),
                    pending: self.buffer.clone(),
                };
            }
            let converted = before_n.to_hiragana();
            let (hiragana_part, romaji_tail) = split_trailing_romaji(&converted);
            if hiragana_part.is_empty() {
                return RomajiOutput {
                    confirmed: String::new(),
                    pending: self.buffer.clone(),
                };
            }
            self.confirmed_so_far.push_str(&hiragana_part);
            self.buffer = format!("{romaji_tail}n");
            return RomajiOutput {
                confirmed: hiragana_part,
                pending: self.buffer.clone(),
            };
        }

        // No 'n' ambiguity: normal wana_kana conversion
        let converted = self.buffer.to_hiragana();
        let (hiragana_part, romaji_tail) = split_trailing_romaji(&converted);

        if hiragana_part.is_empty() {
            RomajiOutput {
                confirmed: String::new(),
                pending: self.buffer.clone(),
            }
        } else {
            self.confirmed_so_far.push_str(&hiragana_part);
            self.buffer = romaji_tail.to_string();

            RomajiOutput {
                confirmed: hiragana_part,
                pending: self.buffer.clone(),
            }
        }
    }

    /// Flush any pending romaji, forcing conversion of whatever is buffered.
    ///
    /// Useful when the user presses Enter or Space to commit.
    pub fn flush_pending(&mut self) -> String {
        if self.buffer.is_empty() {
            return String::new();
        }
        // If the pending 'n' is a residual from "nn", discard it —
        // the ん was already confirmed when "nn" was processed.
        if self.nn_residual {
            self.buffer.clear();
            self.nn_residual = false;
            return String::new();
        }
        // Standalone trailing "n" → "ん" on commit.
        if self.buffer == "n" {
            self.buffer.clear();
            let result = "ん".to_string();
            self.confirmed_so_far.push_str(&result);
            return result;
        }
        let converted = self.buffer.to_hiragana();
        self.buffer.clear();
        self.confirmed_so_far.push_str(&converted);
        converted
    }

    /// Delete the last character from the pending romaji buffer.
    ///
    /// Returns `true` if a character was removed, `false` if the buffer was empty.
    pub fn backspace(&mut self) -> bool {
        if self.buffer.is_empty() {
            return false;
        }
        self.buffer.pop();
        // If the residual 'n' was the only thing in the buffer, clear the flag
        if self.nn_residual && self.buffer.is_empty() {
            self.nn_residual = false;
        }
        true
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.confirmed_so_far.clear();
        self.nn_residual = false;
    }

    /// Get the current pending romaji buffer.
    pub fn pending(&self) -> &str {
        &self.buffer
    }

    /// Get all confirmed hiragana accumulated so far.
    pub fn confirmed_total(&self) -> &str {
        &self.confirmed_so_far
    }
}

/// Split a string into leading hiragana and trailing ASCII (romaji) portions.
///
/// `"おなj"` becomes `("おな", "j")`.
/// `"おなじ"` becomes `("おなじ", "")`.
/// `"sh"` becomes `("", "sh")`.
fn split_trailing_romaji(s: &str) -> (String, &str) {
    let last_kana_end = s
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);

    let hiragana = &s[..last_kana_end];
    let romaji = &s[last_kana_end..];

    (hiragana.to_string(), romaji)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_simple_vowel_confirmed() {
        let mut conv = IncrementalRomaji::new();
        let out = conv.feed('a');
        assert_eq!(out.confirmed, "あ");
        assert_eq!(out.pending, "");
    }

    #[test]
    fn test_feed_consonant_vowel_confirmed() {
        let mut conv = IncrementalRomaji::new();
        let out = conv.feed('k');
        assert_eq!(out.confirmed, "");
        assert_eq!(out.pending, "k");

        let out = conv.feed('a');
        assert_eq!(out.confirmed, "か");
        assert_eq!(out.pending, "");
    }

    #[test]
    fn test_feed_shi_confirmed() {
        let mut conv = IncrementalRomaji::new();
        conv.feed('s');
        conv.feed('h');
        let out = conv.feed('i');
        assert_eq!(out.confirmed, "し");
        assert_eq!(out.pending, "");
    }

    #[test]
    fn test_feed_nn_produces_n() {
        let mut conv = IncrementalRomaji::new();
        conv.feed('n');
        let out = conv.feed('n');
        assert_eq!(out.confirmed, "ん");
    }

    #[test]
    fn test_konna_sequence() {
        let mut conv = IncrementalRomaji::new();
        let mut total = String::new();
        for ch in "konna".chars() {
            total.push_str(&conv.feed(ch).confirmed);
        }
        assert_eq!(total, "こんな");
        assert_eq!(conv.pending(), "");
    }

    #[test]
    fn test_konnichiha_sequence() {
        let mut conv = IncrementalRomaji::new();
        let mut total = String::new();
        for ch in "konnichiha".chars() {
            let out = conv.feed(ch);
            total.push_str(&out.confirmed);
        }
        assert_eq!(total, "こんにちは");
        assert_eq!(conv.pending(), "");
    }

    #[test]
    fn test_watashi_sequence() {
        let mut conv = IncrementalRomaji::new();
        let mut total = String::new();
        for ch in "watashi".chars() {
            total.push_str(&conv.feed(ch).confirmed);
        }
        assert_eq!(total, "わたし");
        assert_eq!(conv.pending(), "");
    }

    #[test]
    fn test_flush_pending_n() {
        // Standalone 'n' not from "nn" — should flush as ん
        let mut conv = IncrementalRomaji::new();
        let out = conv.feed('n');
        assert_eq!(out.confirmed, "");
        assert_eq!(out.pending, "n");
        let flushed = conv.flush_pending();
        assert_eq!(flushed, "ん");
        assert_eq!(conv.pending(), "");
    }

    #[test]
    fn test_nn_commit_produces_single_n() {
        // "nn" + commit should produce exactly ONE ん, not two
        let mut conv = IncrementalRomaji::new();
        let mut total = String::new();
        total.push_str(&conv.feed('n').confirmed);
        total.push_str(&conv.feed('n').confirmed);
        total.push_str(&conv.flush_pending());
        assert_eq!(total, "ん");
    }

    #[test]
    fn test_nn_then_consonant_no_double() {
        // "nn" + 'k' + 'a' should produce んか, not んんか
        let mut conv = IncrementalRomaji::new();
        let mut total = String::new();
        for ch in "nnka".chars() {
            total.push_str(&conv.feed(ch).confirmed);
        }
        assert_eq!(total, "んか");
    }

    #[test]
    fn test_reset_clears_state() {
        let mut conv = IncrementalRomaji::new();
        conv.feed('k');
        conv.reset();
        assert_eq!(conv.pending(), "");
        assert_eq!(conv.confirmed_total(), "");
    }

    #[test]
    fn test_non_ascii_passthrough() {
        let mut conv = IncrementalRomaji::new();
        let out = conv.feed('.');
        // wana_kana converts '.' to '。' in IME mode, but since it's non-ASCII
        // we pass through directly
        assert!(out.confirmed.contains('.'));
        assert_eq!(out.pending, "");
    }

    #[test]
    fn test_split_trailing_romaji_mixed() {
        assert_eq!(split_trailing_romaji("おなj"), ("おな".into(), "j"));
    }

    #[test]
    fn test_split_trailing_romaji_all_kana() {
        assert_eq!(split_trailing_romaji("おなじ"), ("おなじ".into(), ""));
    }

    #[test]
    fn test_split_trailing_romaji_all_ascii() {
        assert_eq!(split_trailing_romaji("sh"), ("".into(), "sh"));
    }

    #[test]
    fn test_split_trailing_romaji_empty() {
        assert_eq!(split_trailing_romaji(""), ("".into(), ""));
    }

    #[test]
    fn test_backspace_removes_pending_romaji() {
        let mut conv = IncrementalRomaji::new();
        conv.feed('k');
        assert_eq!(conv.pending(), "k");
        assert!(conv.backspace());
        assert_eq!(conv.pending(), "");
    }

    #[test]
    fn test_backspace_empty_returns_false() {
        let mut conv = IncrementalRomaji::new();
        assert!(!conv.backspace());
    }

    #[test]
    fn test_backspace_after_nn_residual() {
        let mut conv = IncrementalRomaji::new();
        conv.feed('n');
        conv.feed('n'); // confirms ん, leaves residual 'n'
        assert!(conv.backspace()); // removes the residual 'n'
        assert_eq!(conv.pending(), "");
    }
}
