//! IPAdic dictionary parser with reading-based lookup for kana-kanji conversion.
//!
//! # Design rationale
//!
//! Morphological analyzers like lindera/vibrato match on **surface forms** (漢字).
//! An IME needs the reverse: given a **reading** (ひらがな), find all possible
//! surface forms (漢字) with their costs.
//!
//! This crate parses IPAdic CSV files and builds a reading-indexed dictionary
//! optimized for Common Prefix Search — the core operation for lattice construction.
//!
//! # Dictionary source
//!
//! Uses IPAdic (mecab-ipadic-2.7.0) CSV files, which are freely available.
//! Each line: `surface,left_id,right_id,cost,pos1,pos2,...,reading,pronunciation`
//!
//! The `reading` field is in katakana; we normalize to hiragana for lookup.

use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

/// Errors during dictionary operations.
#[derive(Debug, Error)]
pub enum DictError {
    #[error("Failed to read dictionary file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse line {line}: {reason}")]
    Parse { line: usize, reason: String },
    #[error("No dictionary files found in {0}")]
    NoDictFiles(String),
}

/// A single dictionary entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DictEntry {
    /// Surface form (漢字かな混じり表記, e.g., "今日").
    pub surface: String,
    /// Reading in hiragana (e.g., "きょう").
    pub reading: String,
    /// Left context ID (for connection cost matrix).
    pub left_id: u16,
    /// Right context ID (for connection cost matrix).
    pub right_id: u16,
    /// Word cost (lower = more common).
    pub cost: i32,
}

/// Connection cost matrix for bigram transition costs.
///
/// `matrix[right_id_of_prev][left_id_of_next]` gives the transition cost.
#[derive(Debug)]
pub struct ConnectionCost {
    /// Number of right context IDs.
    right_size: usize,
    /// Number of left context IDs.
    left_size: usize,
    /// Flattened cost matrix.
    costs: Vec<i16>,
}

impl ConnectionCost {
    /// Look up the connection cost between two adjacent morphemes.
    pub fn cost(&self, right_id: u16, left_id: u16) -> i32 {
        let idx = (right_id as usize) * self.left_size + (left_id as usize);
        self.costs.get(idx).copied().unwrap_or(0) as i32
    }

    /// Parse from IPAdic `matrix.def` format.
    pub fn from_reader(reader: impl std::io::BufRead) -> Result<Self, DictError> {
        todo!("M2: Parse matrix.def — first line is 'right_size left_size', rest is 'right left cost'")
    }
}

/// Japanese dictionary indexed by reading (hiragana) for IME use.
#[derive(Debug)]
pub struct Dictionary {
    /// Reading (hiragana) → list of entries, sorted by cost ascending.
    entries: HashMap<String, Vec<DictEntry>>,
}

impl Dictionary {
    /// Load dictionary from a directory containing IPAdic CSV files.
    ///
    /// Expects files like `*.csv` in IPAdic format.
    pub fn load_from_dir(dir: &Path) -> Result<Self, DictError> {
        todo!("M2: glob *.csv, parse each, build reading index")
    }

    /// Load dictionary from a single IPAdic CSV reader.
    pub fn load_from_reader(reader: impl std::io::BufRead) -> Result<Self, DictError> {
        todo!("M2: Parse IPAdic CSV, normalize katakana readings to hiragana, index by reading")
    }

    /// Exact match: all entries whose reading equals `reading`.
    pub fn lookup(&self, reading: &str) -> &[DictEntry] {
        self.entries
            .get(reading)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Common Prefix Search: find all entries whose reading is a prefix of
    /// `input` starting at byte position `start`.
    ///
    /// Returns `(end_byte_position, entries)` pairs, sorted by length ascending.
    /// This is the core operation for lattice construction.
    pub fn common_prefix_search(&self, input: &str, start: usize) -> Vec<(usize, &[DictEntry])> {
        todo!("M2: iterate char boundaries from start, check each prefix in entries map")
    }

    /// Number of unique readings in the dictionary.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the dictionary is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Convert katakana string to hiragana (for normalizing IPAdic readings).
///
/// IPAdic stores readings in katakana (e.g., "キョウ").
/// We normalize to hiragana (e.g., "きょう") for lookup.
fn katakana_to_hiragana(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('\u{30A1}'..='\u{30F6}').contains(&c) {
                // Katakana → Hiragana: subtract 0x60
                char::from_u32(c as u32 - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_katakana_to_hiragana() {
        assert_eq!(katakana_to_hiragana("キョウ"), "きょう");
        assert_eq!(katakana_to_hiragana("トウキョウ"), "とうきょう");
        assert_eq!(katakana_to_hiragana("ワタシ"), "わたし");
    }

    #[test]
    fn test_dict_entry_creation() {
        let entry = DictEntry {
            surface: "今日".into(),
            reading: "きょう".into(),
            left_id: 1,
            right_id: 1,
            cost: 5000,
        };
        assert_eq!(entry.surface, "今日");
        assert_eq!(entry.reading, "きょう");
    }

    #[test]
    fn test_lookup_empty_dict() {
        let dict = Dictionary {
            entries: HashMap::new(),
        };
        assert!(dict.lookup("きょう").is_empty());
    }

    #[test]
    fn test_lookup_existing_reading() {
        let mut entries = HashMap::new();
        entries.insert(
            "きょう".into(),
            vec![
                DictEntry {
                    surface: "今日".into(),
                    reading: "きょう".into(),
                    left_id: 1,
                    right_id: 1,
                    cost: 3000,
                },
                DictEntry {
                    surface: "京".into(),
                    reading: "きょう".into(),
                    left_id: 2,
                    right_id: 2,
                    cost: 7000,
                },
            ],
        );
        let dict = Dictionary { entries };
        assert_eq!(dict.lookup("きょう").len(), 2);
        assert_eq!(dict.lookup("きょう")[0].surface, "今日");
    }
}
