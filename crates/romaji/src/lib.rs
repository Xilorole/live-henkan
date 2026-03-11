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

use wana_kana::to_hiragana::to_hiragana;

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

        let converted = to_hiragana(&self.buffer);
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
        let converted = to_hiragana(&self.buffer);
        self.buffer.clear();
        self.confirmed_so_far.push_str(&converted);
        converted
    }

    /// Reset all internal state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.confirmed_so_far.clear();
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
        let mut conv = IncrementalRomaji::new();
        conv.feed('n');
        let flushed = conv.flush_pending();
        assert_eq!(flushed, "ん");
        assert_eq!(conv.pending(), "");
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
}
