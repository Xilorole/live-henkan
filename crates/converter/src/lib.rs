//! Lattice construction and Viterbi algorithm for Japanese conversion.
//!
//! Given a hiragana string and a dictionary, builds a word lattice
//! (DAG) and finds the minimum-cost path through it.

use dictionary::{ConnectionCost, Dictionary};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("Empty input")]
    EmptyInput,
    #[error("No valid path found for input: {0}")]
    NoPath(String),
}

/// A segment in the conversion result.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// The surface form (converted text).
    pub surface: String,
    /// The reading (hiragana).
    pub reading: String,
    /// The cost of this segment.
    pub cost: i32,
    /// Left context ID (for connection cost lookup).
    pub left_id: u16,
    /// Right context ID (for connection cost lookup).
    pub right_id: u16,
}

/// An edge in the word lattice.
#[derive(Debug, Clone)]
struct LatticeEdge {
    /// End position (byte index in the input string).
    end: usize,
    /// Surface form from the dictionary.
    surface: String,
    /// Reading (hiragana).
    reading: String,
    /// Word cost.
    cost: i32,
    /// Left context ID (for connection cost lookup).
    left_id: u16,
    /// Right context ID (for connection cost lookup).
    right_id: u16,
}

/// Word lattice: a DAG over positions in the input string.
///
/// Positions correspond to byte offsets in the input hiragana string.
/// `edges[i]` contains all edges starting at byte position `i`.
#[derive(Debug)]
pub struct Lattice {
    /// `edges[i]` = edges starting at byte position `i`.
    edges: Vec<Vec<LatticeEdge>>,
    /// Length of input string in bytes.
    input_len: usize,
}

/// Cost for unknown single-character fallback edges (non-hiragana).
const UNKNOWN_WORD_COST: i32 = 30000;
/// Special context ID for unknown words and BOS/EOS.
const UNKNOWN_CONTEXT_ID: u16 = 0;
/// Left context ID for hiragana passthrough edges (incoming connections).
///
/// Uses IPAdic ID 610 (動詞-自立, 連用形), which has universally low
/// connection costs from preceding words (BOS, nouns, verbs). This ensures
/// passthrough edges integrate naturally into the lattice.
const HIRAGANA_LEFT_ID: u16 = 610;
/// Right context ID for hiragana passthrough edges (outgoing connections).
///
/// Uses IPAdic ID 473 (助動詞), which has HIGH connection cost to other
/// passthrough edges (610) at +3374, but LOW connection costs to dictionary
/// word types (nouns ~-254, desu ~-2314, ne ~-3341). This prevents
/// consecutive passthrough edges from chaining cheaply, while allowing
/// passthrough → dictionary transitions to work naturally.
const HIRAGANA_RIGHT_ID: u16 = 473;
/// Extra cost added to katakana surface forms.
///
/// When the input is hiragana, katakana surface entries (e.g. reading し → surface シ)
/// from the dictionary are usually wrong. Penalizing them heavily makes the Viterbi
/// prefer hiragana/kanji surfaces.
const KATAKANA_SURFACE_PENALTY: i32 = 20000;
/// Maximum number of characters for multi-char hiragana passthrough edges.
///
/// Kept at 4 to cover common verb conjugation forms (していて, される, etc.)
/// while preventing very long passthrough edges from beating legitimate
/// multi-segment dictionary paths via connection-cost bonuses.
const HIRAGANA_PASSTHROUGH_MAX_CHARS: usize = 4;

/// Returns true if the string consists entirely of katakana characters.
fn is_all_katakana(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| ('\u{30A0}'..='\u{30FF}').contains(&c))
}

/// Returns true if the character is hiragana.
fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{3096}').contains(&c)
}

/// Returns true if the string consists entirely of hiragana characters.
fn is_all_hiragana(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_hiragana)
}

/// Cost for a hiragana passthrough edge of `n` characters.
///
/// Tuned to work with `HIRAGANA_LEFT_ID` (610) which gives a large
/// connection-cost bonus (typically -5000 to -7000). The word cost must be
/// high enough that passthrough + connection bonus still loses to legitimate
/// dictionary entries, but low enough to beat wrong kanji segmentations.
fn hiragana_passthrough_cost(n: usize) -> i32 {
    7000 + 500 * (n as i32 - 1)
}

impl Lattice {
    /// Build a lattice from a hiragana input string using the given dictionary.
    ///
    /// For each position in the input, performs a common prefix search and adds
    /// edges for all dictionary matches. Also inserts single-character
    /// fallback edges to ensure every position is reachable.
    pub fn build(input: &str, dict: &Dictionary) -> Self {
        let input_len = input.len();
        let mut edges: Vec<Vec<LatticeEdge>> = vec![Vec::new(); input_len + 1];

        // For each starting position, find all dictionary entries
        let mut pos = 0;
        for ch in input.chars() {
            let char_len = ch.len_utf8();

            // Dictionary matches via common prefix search
            let matches = dict.common_prefix_search(input, pos);
            for (end_pos, entries) in &matches {
                for entry in *entries {
                    // Penalize katakana surface forms — the input is hiragana,
                    // so katakana surfaces (e.g. シ, タイ, マス) are almost
                    // always wrong candidates.
                    let cost = if is_all_katakana(&entry.surface) {
                        entry.cost.saturating_add(KATAKANA_SURFACE_PENALTY)
                    } else {
                        entry.cost
                    };
                    edges[pos].push(LatticeEdge {
                        end: *end_pos,
                        surface: entry.surface.clone(),
                        reading: entry.reading.clone(),
                        cost,
                        left_id: entry.left_id,
                        right_id: entry.right_id,
                    });
                }
            }

            if is_hiragana(ch) {
                // Hiragana passthrough edges — allows the Viterbi to keep
                // hiragana characters unconverted, which is essential for
                // particles, verb conjugation suffixes, and other function
                // words that IPAdic lacks as standalone entries.
                let ch_str = input[pos..pos + char_len].to_string();

                // Single-char passthrough (skip if dict already has this
                // exact hiragana surface — e.g. に as 助詞)
                let has_hiragana_surface = edges[pos]
                    .iter()
                    .any(|e| e.surface == ch_str && e.end == pos + char_len);
                if !has_hiragana_surface {
                    edges[pos].push(LatticeEdge {
                        end: pos + char_len,
                        surface: ch_str.clone(),
                        reading: ch_str,
                        cost: hiragana_passthrough_cost(1),
                        left_id: HIRAGANA_LEFT_ID,
                        right_id: HIRAGANA_RIGHT_ID,
                    });
                }

                // Multi-char passthrough: look ahead for consecutive hiragana.
                // This creates edges like "して", "していて" that can compete
                // with wrong kanji segmentations when verb conjugation forms
                // are absent from the dictionary.
                let mut end_byte = char_len;
                let mut n = 1usize;
                for next_ch in input[pos + char_len..].chars() {
                    if !is_hiragana(next_ch) || n >= HIRAGANA_PASSTHROUGH_MAX_CHARS {
                        break;
                    }
                    n += 1;
                    end_byte += next_ch.len_utf8();
                    let substr = input[pos..pos + end_byte].to_string();
                    edges[pos].push(LatticeEdge {
                        end: pos + end_byte,
                        surface: substr.clone(),
                        reading: substr,
                        cost: hiragana_passthrough_cost(n),
                        left_id: HIRAGANA_LEFT_ID,
                        right_id: HIRAGANA_RIGHT_ID,
                    });
                }
            } else {
                // Non-hiragana: add unknown word fallback if no single-char
                // dictionary match exists.
                let has_single_char_match = matches.iter().any(|(end, _)| *end == pos + char_len);
                if !has_single_char_match {
                    let ch_str: String = input[pos..pos + char_len].to_string();
                    edges[pos].push(LatticeEdge {
                        end: pos + char_len,
                        surface: ch_str.clone(),
                        reading: ch_str,
                        cost: UNKNOWN_WORD_COST,
                        left_id: UNKNOWN_CONTEXT_ID,
                        right_id: UNKNOWN_CONTEXT_ID,
                    });
                }
            }

            pos += char_len;
        }

        Lattice { edges, input_len }
    }

    /// Find the minimum-cost path through the lattice using the Viterbi algorithm.
    ///
    /// Uses both word costs (unigram) and connection costs (bigram) if provided.
    /// `initial_right_id` sets the right context ID at position 0 (BOS). When
    /// converting text that follows a previously committed segment, pass that
    /// segment's `right_id` to preserve connection-cost continuity.
    pub fn find_best_path(
        &self,
        conn: Option<&ConnectionCost>,
        initial_right_id: u16,
    ) -> Result<Vec<Segment>, ConvertError> {
        if self.input_len == 0 {
            return Err(ConvertError::EmptyInput);
        }

        // Viterbi forward pass
        // best_cost[i] = minimum total cost to reach position i
        // back_ptr[i] = (start_position, edge_index) of the best edge ending at i
        let n = self.input_len + 1;
        let mut best_cost: Vec<i64> = vec![i64::MAX; n];
        let mut back_ptr: Vec<Option<(usize, usize)>> = vec![None; n];
        // Track the right_id of the best edge ending at each position (for bigram cost)
        let mut best_right_id: Vec<u16> = vec![UNKNOWN_CONTEXT_ID; n];

        best_cost[0] = 0;
        best_right_id[0] = initial_right_id;

        for start in 0..self.input_len {
            if best_cost[start] == i64::MAX {
                continue; // This position is unreachable
            }

            for (edge_idx, edge) in self.edges[start].iter().enumerate() {
                let connection_cost = match conn {
                    Some(c) => c.cost(best_right_id[start], edge.left_id) as i64,
                    None => 0,
                };
                let total_cost = best_cost[start] + edge.cost as i64 + connection_cost;

                if total_cost < best_cost[edge.end] {
                    best_cost[edge.end] = total_cost;
                    back_ptr[edge.end] = Some((start, edge_idx));
                    best_right_id[edge.end] = edge.right_id;
                }
            }
        }

        // Check if end is reachable
        if best_cost[self.input_len] == i64::MAX {
            return Err(ConvertError::NoPath("unreachable end".into()));
        }

        // Backward trace
        let mut segments = Vec::new();
        let mut pos = self.input_len;
        while pos > 0 {
            let (start, edge_idx) =
                back_ptr[pos].ok_or_else(|| ConvertError::NoPath("broken back pointer".into()))?;
            let edge = &self.edges[start][edge_idx];
            segments.push(Segment {
                surface: edge.surface.clone(),
                reading: edge.reading.clone(),
                cost: edge.cost,
                left_id: edge.left_id,
                right_id: edge.right_id,
            });
            pos = start;
        }

        segments.reverse();
        Ok(segments)
    }
}

/// High-level conversion function (unigram only, no connection costs).
pub fn convert(input: &str, dict: &Dictionary) -> Result<Vec<Segment>, ConvertError> {
    if input.is_empty() {
        return Err(ConvertError::EmptyInput);
    }
    let lattice = Lattice::build(input, dict);
    lattice.find_best_path(None, UNKNOWN_CONTEXT_ID)
}

/// High-level conversion function with connection costs (bigram).
pub fn convert_with_conn(
    input: &str,
    dict: &Dictionary,
    conn: &ConnectionCost,
) -> Result<Vec<Segment>, ConvertError> {
    convert_with_conn_ctx(input, dict, conn, UNKNOWN_CONTEXT_ID)
}

/// High-level conversion with connection costs and initial context.
///
/// `initial_right_id` is the `right_id` of the last segment that preceded
/// this input (e.g. from a previously auto-committed portion). Pass `0`
/// for beginning-of-sentence.
pub fn convert_with_conn_ctx(
    input: &str,
    dict: &Dictionary,
    conn: &ConnectionCost,
    initial_right_id: u16,
) -> Result<Vec<Segment>, ConvertError> {
    if input.is_empty() {
        return Err(ConvertError::EmptyInput);
    }
    let lattice = Lattice::build(input, dict);
    lattice.find_best_path(Some(conn), initial_right_id)
}

/// Retrieve candidate surface forms for a given hiragana reading.
///
/// Returns all dictionary entries matching the reading, plus a hiragana
/// passthrough candidate if no exact hiragana entry exists. Results are
/// sorted by cost (ascending) and deduplicated by surface form.
pub fn candidates_for_reading(reading: &str, dict: &Dictionary) -> Vec<Segment> {
    use std::collections::HashSet;

    let mut candidates = Vec::new();

    let entries = dict.lookup(reading);
    for entry in entries {
        let cost = if is_all_katakana(&entry.surface) {
            entry.cost.saturating_add(KATAKANA_SURFACE_PENALTY)
        } else {
            entry.cost
        };
        candidates.push(Segment {
            surface: entry.surface.clone(),
            reading: entry.reading.clone(),
            cost,
            left_id: entry.left_id,
            right_id: entry.right_id,
        });
    }

    // Sort by cost ascending
    candidates.sort_by_key(|c| c.cost);

    // Deduplicate by surface form (keep first = lowest cost)
    let mut seen = HashSet::new();
    candidates.retain(|c| seen.insert(c.surface.clone()));

    // Always include hiragana passthrough as a fallback
    if !seen.contains(reading) {
        let cost = if is_all_hiragana(reading) {
            hiragana_passthrough_cost(reading.chars().count())
        } else {
            UNKNOWN_WORD_COST
        };
        let (ctx_left, ctx_right) = if is_all_hiragana(reading) {
            (HIRAGANA_LEFT_ID, HIRAGANA_RIGHT_ID)
        } else {
            (UNKNOWN_CONTEXT_ID, UNKNOWN_CONTEXT_ID)
        };
        candidates.push(Segment {
            surface: reading.to_string(),
            reading: reading.to_string(),
            cost,
            left_id: ctx_left,
            right_id: ctx_right,
        });
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Helper to create a simple test dictionary.
    fn test_dict() -> Dictionary {
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   京,2,2,7000,名詞,固有名詞,*,*,*,*,京,キョウ,キョー\n\
                   教,3,3,6000,名詞,一般,*,*,*,*,教,キョウ,キョー\n\
                   は,10,10,4000,助詞,係助詞,*,*,*,*,は,ハ,ワ\n\
                   今日は,5,5,2500,感動詞,*,*,*,*,*,今日は,キョウハ,キョーワ\n\
                   木,20,20,8000,名詞,一般,*,*,*,*,木,キ,キ\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        Dictionary::load_from_reader(reader).unwrap()
    }

    #[test]
    fn test_segment_structure() {
        let seg = Segment {
            surface: "今日".into(),
            reading: "きょう".into(),
            cost: 3000,
            left_id: 1,
            right_id: 1,
        };
        assert_eq!(seg.surface, "今日");
    }

    #[test]
    fn test_lattice_build_has_edges() {
        let dict = test_dict();
        let lattice = Lattice::build("きょうは", &dict);

        // Position 0 should have edges for "き", "きょう"
        assert!(!lattice.edges[0].is_empty());

        // Verify dictionary entries are found
        let edge_surfaces: Vec<&str> = lattice.edges[0]
            .iter()
            .map(|e| e.surface.as_str())
            .collect();
        assert!(edge_surfaces.contains(&"木")); // "き"
        assert!(edge_surfaces.contains(&"今日")); // "きょう"
    }

    #[test]
    fn test_convert_single_word() {
        let dict = test_dict();
        let result = convert("きょう", &dict).unwrap();
        // Should prefer 今日 (cost=3000) over 京 (cost=7000)
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].surface, "今日");
    }

    #[test]
    fn test_convert_two_segments() {
        let dict = test_dict();
        let result = convert("きょうは", &dict).unwrap();
        // Could be "今日は" (single, cost=2500) or "今日"+"は" (3000+4000=7000)
        // The single entry "今日は" has lower cost
        let surfaces: Vec<&str> = result.iter().map(|s| s.surface.as_str()).collect();
        // "今日は" as a single word (cost=2500) is cheaper than any split
        assert!(surfaces == vec!["今日は"] || surfaces == vec!["今日", "は"]);
    }

    #[test]
    fn test_convert_empty_input() {
        let dict = test_dict();
        assert!(convert("", &dict).is_err());
    }

    #[test]
    fn test_convert_unknown_chars_fallback() {
        let dict = test_dict();
        // "あ" is not in our test dictionary, so it should fall back to single char
        let result = convert("あ", &dict).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].surface, "あ");
    }

    #[test]
    fn test_lattice_always_connected() {
        let dict = test_dict();
        // Even with chars not in dictionary, lattice should be fully connected
        let result = convert("あいう", &dict);
        assert!(result.is_ok());
        let segs = result.unwrap();
        // All chars should be in result
        let combined: String = segs.iter().map(|s| s.surface.as_str()).collect();
        assert_eq!(combined, "あいう");
    }

    #[test]
    fn test_viterbi_picks_lowest_cost() {
        // Build dict where "きょう" has two entries with different costs
        let csv = "今日,1,1,1000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   教,2,2,9000,名詞,一般,*,*,*,*,教,キョウ,キョー\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        let result = convert("きょう", &dict).unwrap();
        assert_eq!(result[0].surface, "今日"); // lower cost wins
    }

    #[test]
    fn test_connection_costs_affect_result() {
        // Build a scenario where connection costs change the optimal path.
        // Use low-cost entries so kanji path beats hiragana passthrough.
        let csv = "今日,1,1,2000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   は,2,2,2000,助詞,係助詞,*,*,*,*,は,ハ,ワ\n\
                   今日は,3,3,6000,感動詞,*,*,*,*,*,今日は,キョウハ,キョーワ\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        // Without connection costs: 今日(2000) + は(2000) = 4000 < 今日は(6000)
        let result = convert("きょうは", &dict).unwrap();
        let surfaces: Vec<&str> = result.iter().map(|s| s.surface.as_str()).collect();
        assert_eq!(surfaces, vec!["今日", "は"]);

        // With high connection cost between 今日→は, single word might win
        let matrix = "4 4\n1 2 5000\n"; // high cost from right_id=1 to left_id=2
        let conn_reader = std::io::BufReader::new(matrix.as_bytes());
        let conn = ConnectionCost::from_reader(conn_reader).unwrap();

        let result = convert_with_conn("きょうは", &dict, &conn).unwrap();
        let surfaces: Vec<&str> = result.iter().map(|s| s.surface.as_str()).collect();
        // Now 今日(2000) + conn(5000) + は(2000) = 9000 > 今日は(6000)
        assert_eq!(surfaces, vec!["今日は"]);
    }

    #[test]
    fn test_candidates_for_reading_basic() {
        let dict = test_dict();
        let candidates = candidates_for_reading("きょう", &dict);
        let surfaces: Vec<&str> = candidates.iter().map(|c| c.surface.as_str()).collect();
        // Should include 今日, 京, 教 (from dict) and きょう (hiragana passthrough)
        assert!(surfaces.contains(&"今日"));
        assert!(surfaces.contains(&"京"));
        assert!(surfaces.contains(&"教"));
        assert!(surfaces.contains(&"きょう"));
        // Sorted by cost: 今日(3000) < 教(6000) < 京(7000) < きょう(30000)
        assert_eq!(candidates[0].surface, "今日");
    }

    #[test]
    fn test_candidates_for_reading_unknown() {
        let dict = test_dict();
        let candidates = candidates_for_reading("ぬ", &dict);
        // Not in dictionary → only hiragana passthrough
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].surface, "ぬ");
    }

    #[test]
    fn test_candidates_for_reading_dedup() {
        // Dict with duplicate surfaces at different costs
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   今日,2,2,5000,名詞,副詞可能,*,*,*,*,今日,キョウ,キョー\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();
        let candidates = candidates_for_reading("きょう", &dict);
        // "今日" should appear only once (lowest cost kept)
        let today_count = candidates.iter().filter(|c| c.surface == "今日").count();
        assert_eq!(today_count, 1);
        assert_eq!(candidates[0].cost, 3000);
    }

    #[test]
    fn test_candidates_for_reading_katakana_penalty() {
        let csv = "キョウ,1,1,2000,名詞,一般,*,*,*,*,キョウ,キョウ,キョー\n\
                   今日,2,2,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();
        let candidates = candidates_for_reading("きょう", &dict);
        // 今日(3000) should come before キョウ(2000+20000=22000)
        assert_eq!(candidates[0].surface, "今日");
    }

    #[test]
    fn test_hiragana_passthrough_single_char() {
        // Dictionary has only kanji for "し" (市, etc.), no hiragana surface.
        // Passthrough should add "し" as an edge.
        let csv = "市,1,1,4998,名詞,一般,*,*,*,*,市,シ,シ\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        let result = convert("し", &dict).unwrap();
        // Passthrough cost(1) = 7000 > 市(4998), so 市 wins (unigram only)
        assert_eq!(result[0].surface, "市");

        // But if kanji cost is higher than passthrough, passthrough wins
        let csv2 = "市,1,1,8000,名詞,一般,*,*,*,*,市,シ,シ\n";
        let dict2 = Dictionary::load_from_reader(std::io::BufReader::new(csv2.as_bytes())).unwrap();
        let result2 = convert("し", &dict2).unwrap();
        assert_eq!(result2[0].surface, "し");
    }

    #[test]
    fn test_hiragana_multichar_passthrough_beats_wrong_segmentation() {
        // Simulates the "していて" problem:
        // Dictionary has 指定(してい, cost=3781) and て(cost=5170),
        // but no verb conjugation form "して" or "していて".
        // The multi-char passthrough "していて" (4 chars, cost=8500) should
        // beat "指定+て" (3781+5170=8951).
        let csv = "指定,1,1,3781,名詞,一般,*,*,*,*,指定,シテイ,シテイ\n\
                   て,2,2,5170,助詞,接続助詞,*,*,*,*,て,テ,テ\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        let result = convert("していて", &dict).unwrap();
        let combined: String = result.iter().map(|s| s.surface.as_str()).collect();
        // Should stay hiragana rather than become "指定て"
        assert_eq!(combined, "していて");
    }

    #[test]
    fn test_hiragana_passthrough_doesnt_break_good_kanji() {
        // Common words should still convert to kanji
        let csv = "今日,1,1,3000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   楽しみ,2,2,4000,名詞,一般,*,*,*,*,楽しみ,タノシミ,タノシミ\n\
                   真,3,3,4000,名詞,一般,*,*,*,*,真,シン,シン\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        // "きょう" → 今日 (3000 < passthrough 8000)
        let result = convert("きょう", &dict).unwrap();
        assert_eq!(result[0].surface, "今日");

        // "たのしみ" → 楽しみ (4000 < passthrough 8500)
        let result2 = convert("たのしみ", &dict).unwrap();
        assert_eq!(result2[0].surface, "楽しみ");

        // "しん" → 真 (4000 < passthrough 7500)
        let result3 = convert("しん", &dict).unwrap();
        assert_eq!(result3[0].surface, "真");
    }

    #[test]
    fn test_candidates_hiragana_passthrough_cost() {
        let csv = "指定,1,1,3781,名詞,一般,*,*,*,*,指定,シテイ,シテイ\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        let candidates = candidates_for_reading("してい", &dict);
        // Should have 指定 and "してい" (hiragana passthrough)
        let surfaces: Vec<&str> = candidates.iter().map(|c| c.surface.as_str()).collect();
        assert!(surfaces.contains(&"指定"));
        assert!(surfaces.contains(&"してい"));

        // Hiragana passthrough cost should use hiragana formula, not 30000
        let passthrough = candidates.iter().find(|c| c.surface == "してい").unwrap();
        assert_eq!(passthrough.cost, hiragana_passthrough_cost(3)); // 8000
    }
}
