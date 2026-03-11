//! Live conversion engine integrating romaji, dictionary, and converter.
//!
//! Processes keystroke-by-keystroke input and produces continuously
//! updated conversion output — the core of "live conversion".

use converter::{candidates_for_reading, convert_with_conn_ctx, Segment};
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

/// Engine operating mode.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineMode {
    /// Normal composing mode — conversion updates on each keystroke.
    Composing,
    /// Candidate selection mode — user is picking from alternatives.
    Selecting,
}

/// A segment for display, with optional active marker.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySegment {
    /// Surface form to display.
    pub surface: String,
    /// Hiragana reading.
    pub reading: String,
    /// Whether this segment is the one being edited.
    pub is_active: bool,
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
    /// Current operating mode.
    mode: EngineMode,
    /// Per-segment candidate lists (populated when entering selection mode).
    segment_candidates: Vec<Vec<Segment>>,
    /// Currently selected candidate index for each segment.
    candidate_indices: Vec<usize>,
    /// Index of the segment currently being edited.
    active_segment: usize,
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
            mode: EngineMode::Composing,
            segment_candidates: Vec::new(),
            candidate_indices: Vec::new(),
            active_segment: 0,
        }
    }

    /// Process a single key input and return the updated output.
    ///
    /// Feeds the character through romaji conversion, accumulates hiragana,
    /// runs the converter on the full hiragana buffer, and returns the result.
    /// When composing text exceeds [`AUTO_COMMIT_THRESHOLD`] characters,
    /// leading segments are auto-committed.
    ///
    /// If the engine is in selection mode, exits selection first, applying
    /// the current candidate choices.
    pub fn on_key(&mut self, ch: char) -> EngineOutput {
        // Exit selection mode on text input, keeping current choices
        if self.mode == EngineMode::Selecting {
            self.apply_selection();
        }
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
    /// Returns the final converted text. If cached segments exist (e.g.
    /// from candidate selection), uses those instead of re-running
    /// conversion.
    pub fn commit(&mut self) -> String {
        // Flush any pending romaji
        let flushed = self.romaji.flush_pending();
        self.hiragana_buf.push_str(&flushed);

        let result = if self.hiragana_buf.is_empty() && self.cached_segments.is_none() {
            String::new()
        } else if let Some(ref segments) = self.cached_segments {
            // Use cached segments (preserves candidate selections)
            segments.iter().map(|s| s.surface.as_str()).collect()
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
        self.mode = EngineMode::Composing;
        self.segment_candidates.clear();
        self.candidate_indices.clear();
        self.active_segment = 0;
    }

    /// Get all committed text so far.
    pub fn committed_total(&self) -> &str {
        &self.committed
    }

    /// Get current hiragana buffer (for debugging/display).
    pub fn hiragana_buffer(&self) -> &str {
        &self.hiragana_buf
    }

    /// Get the current engine mode.
    pub fn mode(&self) -> &EngineMode {
        &self.mode
    }

    /// Enter candidate selection mode.
    ///
    /// Returns `true` if selection mode was entered. Returns `false` if
    /// there is nothing to select (no composing text).
    pub fn enter_selection(&mut self) -> bool {
        let segments = match &self.cached_segments {
            Some(segs) if !segs.is_empty() => segs.clone(),
            _ => return false,
        };

        // Build candidate lists for each segment
        let mut all_candidates = Vec::with_capacity(segments.len());
        let mut indices = Vec::with_capacity(segments.len());

        for seg in &segments {
            let candidates = candidates_for_reading(&seg.reading, &self.dict);
            // Find the index of the current surface in candidates
            let idx = candidates
                .iter()
                .position(|c| c.surface == seg.surface)
                .unwrap_or(0);
            indices.push(idx);
            all_candidates.push(candidates);
        }

        self.segment_candidates = all_candidates;
        self.candidate_indices = indices;
        self.active_segment = 0;
        self.mode = EngineMode::Selecting;
        true
    }

    /// Exit selection mode, discarding candidate choices and
    /// reverting to the Viterbi best path.
    pub fn cancel_selection(&mut self) {
        self.mode = EngineMode::Composing;
        self.segment_candidates.clear();
        self.candidate_indices.clear();
        self.active_segment = 0;
    }

    /// Exit selection mode, applying the current candidate choices
    /// to the cached segments.
    fn apply_selection(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        if let Some(ref mut segments) = self.cached_segments {
            for (i, seg) in segments.iter_mut().enumerate() {
                if i < self.segment_candidates.len() && i < self.candidate_indices.len() {
                    let ci = self.candidate_indices[i];
                    if let Some(chosen) = self.segment_candidates[i].get(ci) {
                        seg.surface = chosen.surface.clone();
                        seg.left_id = chosen.left_id;
                        seg.right_id = chosen.right_id;
                        seg.cost = chosen.cost;
                    }
                }
            }
        }
        self.mode = EngineMode::Composing;
        self.segment_candidates.clear();
        self.candidate_indices.clear();
        self.active_segment = 0;
    }

    /// Confirm the current selection and commit the composed text.
    ///
    /// Applies candidate choices, commits all segments, and resets.
    pub fn confirm_selection(&mut self) -> String {
        self.apply_selection();
        self.commit()
    }

    /// Move to the next candidate for the active segment.
    pub fn next_candidate(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        if let Some(candidates) = self.segment_candidates.get(self.active_segment) {
            if !candidates.is_empty() {
                let idx = &mut self.candidate_indices[self.active_segment];
                *idx = (*idx + 1) % candidates.len();
            }
        }
    }

    /// Move to the previous candidate for the active segment.
    pub fn prev_candidate(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        if let Some(candidates) = self.segment_candidates.get(self.active_segment) {
            if !candidates.is_empty() {
                let idx = &mut self.candidate_indices[self.active_segment];
                *idx = if *idx == 0 {
                    candidates.len() - 1
                } else {
                    *idx - 1
                };
            }
        }
    }

    /// Move to the next segment (rightward).
    pub fn next_segment(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        if self.active_segment + 1 < self.segment_candidates.len() {
            self.active_segment += 1;
        }
    }

    /// Move to the previous segment (leftward).
    pub fn prev_segment(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        if self.active_segment > 0 {
            self.active_segment -= 1;
        }
    }

    /// Get the display segments for the current composing text.
    ///
    /// In selection mode, surfaces reflect the currently selected candidates.
    /// Returns an empty vec if there is no composing text.
    pub fn display_segments(&self) -> Vec<DisplaySegment> {
        let segments = match &self.cached_segments {
            Some(segs) => segs,
            None => return Vec::new(),
        };

        segments
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let surface = if self.mode == EngineMode::Selecting {
                    // Use the selected candidate's surface
                    self.segment_candidates
                        .get(i)
                        .and_then(|cands| cands.get(self.candidate_indices[i]))
                        .map(|c| c.surface.clone())
                        .unwrap_or_else(|| seg.surface.clone())
                } else {
                    seg.surface.clone()
                };
                DisplaySegment {
                    surface,
                    reading: seg.reading.clone(),
                    is_active: self.mode == EngineMode::Selecting && i == self.active_segment,
                }
            })
            .collect()
    }

    /// Get the candidate list for the active segment.
    ///
    /// Returns an empty slice if not in selection mode.
    pub fn current_candidates(&self) -> &[Segment] {
        if self.mode != EngineMode::Selecting {
            return &[];
        }
        self.segment_candidates
            .get(self.active_segment)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the index of the currently selected candidate.
    pub fn active_candidate_index(&self) -> usize {
        if self.mode != EngineMode::Selecting {
            return 0;
        }
        self.candidate_indices
            .get(self.active_segment)
            .copied()
            .unwrap_or(0)
    }

    /// Get the index of the active segment.
    pub fn active_segment_index(&self) -> usize {
        self.active_segment
    }

    /// Extend the active segment by absorbing one character from the next segment.
    ///
    /// Merges the first character of the next segment's reading into the active
    /// segment, then re-looks up candidates for both affected segments.
    /// Does nothing if the active segment is the last one, or if the next
    /// segment has no characters to give.
    pub fn extend_segment(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        let (new_active_reading, remaining_next) = {
            let segments = match self.cached_segments {
                Some(ref segs) => segs,
                None => return,
            };
            let idx = self.active_segment;
            if idx + 1 >= segments.len() {
                return;
            }

            let next_reading = &segments[idx + 1].reading;
            let first_char = match next_reading.chars().next() {
                Some(c) => c,
                None => return,
            };
            let first_char_len = first_char.len_utf8();
            let remaining = &next_reading[first_char_len..];

            let new_active = format!("{}{}", segments[idx].reading, first_char);
            let remaining_opt = if remaining.is_empty() {
                None
            } else {
                Some(remaining.to_string())
            };
            (new_active, remaining_opt)
        };

        self.rebuild_segments_after_resize(
            self.active_segment,
            &new_active_reading,
            remaining_next,
        );
    }

    /// Shrink the active segment by moving its last character to the next segment.
    ///
    /// Removes the last character from the active segment's reading and
    /// prepends it to the next segment. If the active segment would become
    /// empty, does nothing. If there is no next segment, a new one is
    /// created.
    pub fn shrink_segment(&mut self) {
        if self.mode != EngineMode::Selecting {
            return;
        }
        let (new_active_reading, new_next_reading) = {
            let segments = match self.cached_segments {
                Some(ref segs) => segs,
                None => return,
            };
            let idx = self.active_segment;
            let reading = &segments[idx].reading;

            // Must have at least 2 characters to shrink
            if reading.chars().count() < 2 {
                return;
            }

            let last_char = reading.chars().last().unwrap();
            let last_char_len = last_char.len_utf8();
            let new_active = reading[..reading.len() - last_char_len].to_string();

            let new_next = if idx + 1 < segments.len() {
                format!("{}{}", last_char, segments[idx + 1].reading)
            } else {
                last_char.to_string()
            };
            (new_active, new_next)
        };

        self.rebuild_segments_after_resize(
            self.active_segment,
            &new_active_reading,
            Some(new_next_reading),
        );
    }

    /// Rebuild cached segments and candidate lists after a segment boundary change.
    ///
    /// `idx` is the active segment index. `new_active_reading` is the new reading
    /// for the active segment. `new_next_reading` is the new reading for the
    /// segment after it (`None` means the next segment was fully absorbed).
    fn rebuild_segments_after_resize(
        &mut self,
        idx: usize,
        new_active_reading: &str,
        new_next_reading: Option<String>,
    ) {
        let segments = match self.cached_segments {
            Some(ref mut segs) => segs,
            None => return,
        };

        // Determine best candidate for the new active reading
        let active_candidates = candidates_for_reading(new_active_reading, &self.dict);
        let best_active = active_candidates.first().cloned().unwrap_or(Segment {
            surface: new_active_reading.to_string(),
            reading: new_active_reading.to_string(),
            cost: 30000,
            left_id: 0,
            right_id: 0,
        });

        // Update the active segment
        segments[idx] = best_active;

        match new_next_reading {
            Some(next_reading) => {
                let next_candidates = candidates_for_reading(&next_reading, &self.dict);
                let best_next = next_candidates.first().cloned().unwrap_or(Segment {
                    surface: next_reading.clone(),
                    reading: next_reading.clone(),
                    cost: 30000,
                    left_id: 0,
                    right_id: 0,
                });

                if idx + 1 < segments.len() {
                    // Replace existing next segment
                    segments[idx + 1] = best_next;
                } else {
                    // Insert a new segment after the active one
                    segments.push(best_next);
                }

                // Rebuild candidate lists for both affected segments
                self.segment_candidates[idx] = active_candidates;
                self.candidate_indices[idx] = 0;

                let next_cands = candidates_for_reading(&next_reading, &self.dict);
                if idx + 1 < self.segment_candidates.len() {
                    self.segment_candidates[idx + 1] = next_cands;
                    self.candidate_indices[idx + 1] = 0;
                } else {
                    self.segment_candidates.push(next_cands);
                    self.candidate_indices.push(0);
                }
            }
            None => {
                // Next segment was fully absorbed — remove it
                if idx + 1 < segments.len() {
                    segments.remove(idx + 1);
                }
                if idx + 1 < self.segment_candidates.len() {
                    self.segment_candidates.remove(idx + 1);
                    self.candidate_indices.remove(idx + 1);
                }

                // Rebuild candidate list for the active segment
                self.segment_candidates[idx] = active_candidates;
                self.candidate_indices[idx] = 0;
            }
        }
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

    #[test]
    fn test_engine_enter_selection_empty() {
        let mut engine = test_engine();
        // No composing text → cannot enter selection
        assert!(!engine.enter_selection());
        assert_eq!(*engine.mode(), EngineMode::Composing);
    }

    #[test]
    fn test_engine_enter_selection_with_composing() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        // hiragana_buf = "きょう", composing = "今日"
        assert!(engine.enter_selection());
        assert_eq!(*engine.mode(), EngineMode::Selecting);

        // Should have candidates for "きょう"
        let candidates = engine.current_candidates();
        assert!(!candidates.is_empty());
        // First candidate should be the Viterbi pick (今日)
        assert_eq!(candidates[engine.active_candidate_index()].surface, "今日");
    }

    #[test]
    fn test_engine_next_prev_candidate() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        let first = engine.active_candidate_index();
        engine.next_candidate();
        let second = engine.active_candidate_index();
        assert_ne!(first, second);

        engine.prev_candidate();
        assert_eq!(engine.active_candidate_index(), first);
    }

    #[test]
    fn test_engine_candidate_wraps_around() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        let count = engine.current_candidates().len();
        // Keep pressing next to wrap around
        for _ in 0..count {
            engine.next_candidate();
        }
        // Should wrap back to original
        assert_eq!(engine.active_candidate_index(), 0);
    }

    #[test]
    fn test_engine_next_prev_segment() {
        let mut engine = test_engine();
        // Type "kyouha" → "きょうは" → two segments "今日" + "は"
        for ch in "kyouha".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();
        assert_eq!(engine.active_segment_index(), 0);

        let seg_count = engine.display_segments().len();
        if seg_count > 1 {
            engine.next_segment();
            assert_eq!(engine.active_segment_index(), 1);
            engine.prev_segment();
            assert_eq!(engine.active_segment_index(), 0);
        }
    }

    #[test]
    fn test_engine_display_segments_in_selection() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        let segs = engine.display_segments();
        assert!(!segs.is_empty());
        assert!(segs[0].is_active);
        assert_eq!(segs[0].surface, "今日");
    }

    #[test]
    fn test_engine_confirm_selection() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();
        engine.next_candidate(); // switch from 今日 to another candidate

        let committed = engine.confirm_selection();
        assert!(!committed.is_empty());
        assert_eq!(*engine.mode(), EngineMode::Composing);
        assert!(engine.hiragana_buffer().is_empty());
    }

    #[test]
    fn test_engine_cancel_selection() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();
        engine.cancel_selection();

        assert_eq!(*engine.mode(), EngineMode::Composing);
        // Composing should revert to Viterbi best
        let segs = engine.display_segments();
        assert!(!segs.is_empty());
        assert_eq!(segs[0].surface, "今日");
    }

    #[test]
    fn test_engine_on_key_exits_selection() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();
        assert_eq!(*engine.mode(), EngineMode::Selecting);

        // Typing a new character should exit selection
        engine.on_key('h');
        assert_eq!(*engine.mode(), EngineMode::Composing);
    }

    #[test]
    fn test_engine_confirm_preserves_selected_candidate() {
        // Build engine with multiple candidates for "きょう"
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   京,2,2,7000,名詞,固有名詞,*,*,*,*,京,キョウ,キョー\n\
                   は,10,10,4000,助詞,係助詞,*,*,*,*,は,ハ,ワ\n";
        let dict = Dictionary::load_from_reader(std::io::BufReader::new(csv.as_bytes())).unwrap();
        let matrix = "12 12\n";
        let conn = ConnectionCost::from_reader(std::io::BufReader::new(matrix.as_bytes())).unwrap();
        let mut engine = LiveEngine::new(dict, conn);

        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        // Composing should be "今日" (lowest cost)
        engine.enter_selection();
        // Switch to "京" (next candidate)
        engine.next_candidate();
        let segs = engine.display_segments();
        assert_eq!(segs[0].surface, "京");

        // Confirm should commit "京", not revert to "今日"
        let committed = engine.confirm_selection();
        assert_eq!(committed, "京");
    }

    #[test]
    fn test_engine_extend_segment() {
        // Force 2 segments: 今日は(9000) > 今日(3000) + は(4000) = 7000
        // Then extending first should merge "きょう" + "は" → "きょうは"
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   今日は,5,5,2500,感動詞,*,*,*,*,*,今日は,キョウハ,キョーワ\n\
                   は,10,10,4000,助詞,係助詞,*,*,*,*,は,ハ,ワ\n";
        let dict = Dictionary::load_from_reader(std::io::BufReader::new(csv.as_bytes())).unwrap();
        // High connection cost between 今日→は to NOT force single segment
        // (doesn't matter, Viterbi picks 今日は as single segment at cost 2500)
        let matrix = "12 12\n";
        let conn = ConnectionCost::from_reader(std::io::BufReader::new(matrix.as_bytes())).unwrap();
        let mut engine = LiveEngine::new(dict, conn);

        for ch in "kyouha".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        let segs_before = engine.display_segments();
        // Viterbi picks "今日は" as a single segment (cost 2500)
        // So we shrink it first to create two segments, then extend
        engine.shrink_segment();
        let segs_after_shrink = engine.display_segments();
        assert!(
            segs_after_shrink.len() > segs_before.len(),
            "shrink should create more segments"
        );

        // Now extend back — should merge the char back
        let reading_before_extend = segs_after_shrink[0].reading.clone();
        engine.extend_segment();
        let segs_after_extend = engine.display_segments();
        assert!(segs_after_extend[0].reading.len() > reading_before_extend.len());
    }

    #[test]
    fn test_engine_shrink_segment() {
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   は,10,10,4000,助詞,係助詞,*,*,*,*,は,ハ,ワ\n";
        let dict = Dictionary::load_from_reader(std::io::BufReader::new(csv.as_bytes())).unwrap();
        let matrix = "12 12\n";
        let conn = ConnectionCost::from_reader(std::io::BufReader::new(matrix.as_bytes())).unwrap();
        let mut engine = LiveEngine::new(dict, conn);

        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        let segs_before = engine.display_segments();
        let reading_before = segs_before[0].reading.clone();

        engine.shrink_segment();

        let segs_after = engine.display_segments();
        // Active segment reading should be shorter
        assert!(segs_after[0].reading.len() < reading_before.len());
        // There should be a next segment with the displaced character
        assert!(segs_after.len() > segs_before.len());
    }

    #[test]
    fn test_engine_shrink_single_char_noop() {
        let mut engine = test_engine();
        // "は" → single-char segment
        for ch in "ha".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        let segs_before = engine.display_segments();
        engine.shrink_segment();
        let segs_after = engine.display_segments();
        // Should not change (can't shrink single char)
        assert_eq!(segs_before, segs_after);
    }

    #[test]
    fn test_engine_extend_last_segment_noop() {
        let mut engine = test_engine();
        for ch in "kyou".chars() {
            engine.on_key(ch);
        }
        engine.enter_selection();

        // Only one segment, extending should be a no-op
        let segs_before = engine.display_segments();
        engine.extend_segment();
        let segs_after = engine.display_segments();
        assert_eq!(segs_before, segs_after);
    }
}
