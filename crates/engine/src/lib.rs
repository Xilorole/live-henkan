//! Live conversion engine integrating romaji, dictionary, and converter.
//!
//! Processes keystroke-by-keystroke input and produces continuously
//! updated conversion output — the core of "live conversion".

use converter::{convert_with_conn_ctx, Segment};
use dictionary::{ConnectionCost, Dictionary};
use romaji::IncrementalRomaji;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Dictionary error: {0}")]
    Dict(#[from] dictionary::DictError),
    #[error("Conversion error: {0}")]
    Convert(#[from] converter::ConvertError),
}

/// Output produced after each keystroke.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineOutput {
    /// Text that has been committed (finalized).
    pub committed: String,
    /// Text currently being composed (live conversion result).
    pub composing: String,
    /// Raw romaji input not yet converted to hiragana.
    pub raw_pending: String,
}

/// Number of composing characters to keep before auto-committing leading segments.
const AUTO_COMMIT_THRESHOLD: usize = 20;

/// The live conversion engine.
///
/// Integrates romaji input, dictionary lookup, and lattice-based conversion
/// into a single keystroke-driven API.
pub struct LiveEngine {
    romaji: IncrementalRomaji,
    dict: Dictionary,
    conn: ConnectionCost,
    /// Accumulated hiragana buffer (composed but not committed).
    hiragana_buf: String,
    /// Committed output so far.
    committed: String,
    /// Right context ID of the last auto-committed segment.
    /// Used to seed the Viterbi so that re-conversion of the remaining
    /// buffer produces the same segmentation as the full buffer.
    last_right_id: u16,
    /// Cached conversion segments for the current hiragana_buf.
    /// Avoids re-running conversion when only pending romaji changes.
    cached_segments: Option<Vec<Segment>>,
    /// The hiragana input that produced `cached_segments`.
    cached_hiragana: String,
}

impl LiveEngine {
    /// Create a new engine with the given dictionary and connection costs.
    pub fn new(dict: Dictionary, conn: ConnectionCost) -> Self {
        Self {
            romaji: IncrementalRomaji::new(),
            dict,
            conn,
            hiragana_buf: String::new(),
            committed: String::new(),
            last_right_id: 0,
            cached_segments: None,
            cached_hiragana: String::new(),
        }
    }

    /// Process a single key input and return the updated output.
    ///
    /// Feeds the character through romaji conversion, accumulates hiragana,
    /// runs the converter on the full hiragana buffer, and returns the result.
    /// When composing text exceeds [`AUTO_COMMIT_THRESHOLD`] characters,
    /// leading segments are auto-committed.
    pub fn on_key(&mut self, ch: char) -> EngineOutput {
        let romaji_out = self.romaji.feed(ch);
        self.hiragana_buf.push_str(&romaji_out.confirmed);

        if self.hiragana_buf.is_empty() {
            return EngineOutput {
                committed: String::new(),
                composing: String::new(),
                raw_pending: romaji_out.pending,
            };
        }

        // If hiragana_buf hasn't changed, return cached composing
        if self.hiragana_buf == self.cached_hiragana {
            if let Some(ref segs) = self.cached_segments {
                let composing: String = segs.iter().map(|s| s.surface.as_str()).collect();
                return EngineOutput {
                    committed: String::new(),
                    composing,
                    raw_pending: romaji_out.pending,
                };
            }
        }

        let segments = match convert_with_conn_ctx(
            &self.hiragana_buf,
            &self.dict,
            &self.conn,
            self.last_right_id,
        ) {
            Ok(segments) => segments,
            Err(_) => {
                return EngineOutput {
                    committed: String::new(),
                    composing: self.hiragana_buf.clone(),
                    raw_pending: romaji_out.pending,
                };
            }
        };

        let total_chars: usize = segments.iter().map(|s| s.surface.chars().count()).sum();

        if total_chars > AUTO_COMMIT_THRESHOLD {
            let commit_target = total_chars - AUTO_COMMIT_THRESHOLD;
            let mut auto_committed = String::new();
            let mut remaining_segments: Vec<Segment> = Vec::new();
            let mut remaining_hiragana = String::new();
            let mut counted = 0;
            let mut last_committed_right_id = self.last_right_id;

            for seg in &segments {
                let seg_chars = seg.surface.chars().count();
                if counted < commit_target {
                    auto_committed.push_str(&seg.surface);
                    last_committed_right_id = seg.right_id;
                    counted += seg_chars;
                } else {
                    remaining_hiragana.push_str(&seg.reading);
                    remaining_segments.push(seg.clone());
                }
            }

            self.committed.push_str(&auto_committed);
            self.hiragana_buf = remaining_hiragana.clone();
            self.last_right_id = last_committed_right_id;
            let remaining_composing: String = remaining_segments
                .iter()
                .map(|s| s.surface.as_str())
                .collect();
            self.cached_hiragana = remaining_hiragana;
            self.cached_segments = Some(remaining_segments);

            return EngineOutput {
                committed: auto_committed,
                composing: remaining_composing,
                raw_pending: romaji_out.pending,
            };
        }

        let composing: String = segments.iter().map(|s| s.surface.as_str()).collect();
        self.cached_hiragana = self.hiragana_buf.clone();
        self.cached_segments = Some(segments);

        EngineOutput {
            committed: String::new(),
            composing,
            raw_pending: romaji_out.pending,
        }
    }

    /// Commit the current composition and reset for new input.
    ///
    /// Returns the final converted text.
    pub fn commit(&mut self) -> String {
        // Flush any pending romaji
        let flushed = self.romaji.flush_pending();
        self.hiragana_buf.push_str(&flushed);

        let result = if self.hiragana_buf.is_empty() {
            String::new()
        } else {
            match convert_with_conn_ctx(
                &self.hiragana_buf,
                &self.dict,
                &self.conn,
                self.last_right_id,
            ) {
                Ok(segments) => segments.iter().map(|s| s.surface.as_str()).collect(),
                Err(_) => self.hiragana_buf.clone(),
            }
        };

        self.committed.push_str(&result);
        self.hiragana_buf.clear();
        self.last_right_id = 0;
        self.cached_segments = None;
        self.cached_hiragana.clear();
        result
    }

    /// Delete one unit from the composition.
    ///
    /// First removes pending romaji. If there is none, removes the last
    /// hiragana character from the buffer and re-runs conversion.
    pub fn backspace(&mut self) -> EngineOutput {
        if self.romaji.backspace() {
            // Removed a pending romaji character
            return self.current_output();
        }
        // No pending romaji — delete last hiragana character
        if self.hiragana_buf.pop().is_some() {
            return self.current_output();
        }
        // Nothing to delete
        EngineOutput {
            committed: String::new(),
            composing: String::new(),
            raw_pending: String::new(),
        }
    }

    /// Build an [`EngineOutput`] from the current state (no auto-commit).
    fn current_output(&mut self) -> EngineOutput {
        let composing = if self.hiragana_buf.is_empty() {
            self.cached_segments = None;
            self.cached_hiragana.clear();
            String::new()
        } else {
            match convert_with_conn_ctx(
                &self.hiragana_buf,
                &self.dict,
                &self.conn,
                self.last_right_id,
            ) {
                Ok(segments) => {
                    let s: String = segments.iter().map(|s| s.surface.as_str()).collect();
                    self.cached_hiragana = self.hiragana_buf.clone();
                    self.cached_segments = Some(segments);
                    s
                }
                Err(_) => {
                    self.cached_segments = None;
                    self.cached_hiragana.clear();
                    self.hiragana_buf.clone()
                }
            }
        };
        EngineOutput {
            committed: String::new(),
            composing,
            raw_pending: self.romaji.pending().to_string(),
        }
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        self.romaji.reset();
        self.hiragana_buf.clear();
        self.committed.clear();
        self.last_right_id = 0;
        self.cached_segments = None;
        self.cached_hiragana.clear();
    }

    /// Get all committed text so far.
    pub fn committed_total(&self) -> &str {
        &self.committed
    }

    /// Get current hiragana buffer (for debugging/display).
    pub fn hiragana_buffer(&self) -> &str {
        &self.hiragana_buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine() -> LiveEngine {
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   は,10,10,4000,助詞,係助詞,*,*,*,*,は,ハ,ワ\n\
                   私,11,11,2000,名詞,代名詞,*,*,*,*,私,ワタシ,ワタシ\n";
        let dict = Dictionary::load_from_reader(std::io::BufReader::new(csv.as_bytes())).unwrap();
        let matrix = "12 12\n";
        let conn = ConnectionCost::from_reader(std::io::BufReader::new(matrix.as_bytes())).unwrap();
        LiveEngine::new(dict, conn)
    }

    #[test]
    fn test_engine_output_structure() {
        let output = EngineOutput {
            committed: String::new(),
            composing: "今日は".into(),
            raw_pending: "".into(),
        };
        assert_eq!(output.composing, "今日は");
    }

    #[test]
    fn test_engine_romaji_pending() {
        let mut engine = test_engine();
        let out = engine.on_key('k');
        assert_eq!(out.raw_pending, "k");
        assert_eq!(out.composing, "");
    }

    #[test]
    fn test_engine_simple_vowel() {
        let mut engine = test_engine();
        let out = engine.on_key('a');
        // "あ" not in dictionary, so composing falls back to hiragana
        assert_eq!(out.composing, "あ");
        assert_eq!(out.raw_pending, "");
    }

    #[test]
    fn test_engine_kyou_converts() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        // After typing "kyou" → hiragana "きょう" → should convert to "今日"
        let out = engine.on_key('h'); // 'h' starts pending for next char
                                      // hiragana_buf should be "きょう", pending "h"
        assert_eq!(out.composing, "今日");
        assert_eq!(out.raw_pending, "h");
    }

    #[test]
    fn test_engine_commit() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        let committed = engine.commit();
        assert_eq!(committed, "今日");
    }

    #[test]
    fn test_engine_reset() {
        let mut engine = test_engine();
        engine.on_key('a');
        engine.reset();
        let out = engine.on_key('a');
        assert_eq!(out.composing, "あ");
    }

    #[test]
    fn test_engine_watashi_converts() {
        let mut engine = test_engine();
        for ch in "watashi".chars() {
            engine.on_key(ch);
        }
        let committed = engine.commit();
        assert_eq!(committed, "私");
    }

    #[test]
    fn test_engine_backspace_removes_pending_romaji() {
        let mut engine = test_engine();
        engine.on_key('k'); // pending "k"
        let out = engine.backspace();
        assert_eq!(out.raw_pending, "");
        assert_eq!(out.composing, "");
    }

    #[test]
    fn test_engine_backspace_removes_last_hiragana() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        // hiragana_buf = "きょう", composing = "今日"
        let out = engine.backspace();
        // removed 'う' → hiragana_buf = "きょ"
        assert_eq!(engine.hiragana_buffer(), "きょ");
        assert!(!out.composing.is_empty()); // should still have some composing
    }

    #[test]
    fn test_engine_backspace_empty() {
        let mut engine = test_engine();
        let out = engine.backspace();
        assert_eq!(out.composing, "");
        assert_eq!(out.raw_pending, "");
    }
}
