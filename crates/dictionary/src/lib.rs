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
use std::io::BufRead;
use std::path::Path;

use encoding_rs::EUC_JP;
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
    /// Number of right context IDs (used for validation).
    #[allow(dead_code)]
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
    ///
    /// First line: `right_size left_size`
    /// Remaining lines: `right_id left_id cost`
    pub fn from_reader(reader: impl BufRead) -> Result<Self, DictError> {
        let mut lines = reader.lines();

        let header = lines
            .next()
            .ok_or_else(|| DictError::Parse {
                line: 1,
                reason: "empty matrix.def".into(),
            })?
            .map_err(DictError::Io)?;

        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(DictError::Parse {
                line: 1,
                reason: format!("expected 'right_size left_size', got '{header}'"),
            });
        }
        let right_size: usize = parts[0].parse().map_err(|_| DictError::Parse {
            line: 1,
            reason: "invalid right_size".into(),
        })?;
        let left_size: usize = parts[1].parse().map_err(|_| DictError::Parse {
            line: 1,
            reason: "invalid left_size".into(),
        })?;

        let mut costs = vec![0i16; right_size * left_size];

        for (i, line_result) in lines.enumerate() {
            let line = line_result.map_err(DictError::Io)?;
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 3 {
                continue;
            }
            let right_id: usize = parts[0].parse().map_err(|_| DictError::Parse {
                line: i + 2,
                reason: "invalid right_id".into(),
            })?;
            let left_id: usize = parts[1].parse().map_err(|_| DictError::Parse {
                line: i + 2,
                reason: "invalid left_id".into(),
            })?;
            let cost: i16 = parts[2].parse().map_err(|_| DictError::Parse {
                line: i + 2,
                reason: "invalid cost".into(),
            })?;

            if right_id < right_size && left_id < left_size {
                costs[right_id * left_size + left_id] = cost;
            }
        }

        Ok(Self {
            right_size,
            left_size,
            costs,
        })
    }
}

/// Japanese dictionary indexed by reading (hiragana) for IME use.
#[derive(Debug)]
pub struct Dictionary {
    /// Reading (hiragana) → list of entries, sorted by cost ascending.
    entries: HashMap<String, Vec<DictEntry>>,
}

impl Dictionary {
    /// Load dictionary from a directory containing IPAdic CSV files (EUC-JP encoded).
    ///
    /// Expects files like `*.csv` in IPAdic format.
    pub fn load_from_dir(dir: &Path) -> Result<Self, DictError> {
        let mut all_entries: HashMap<String, Vec<DictEntry>> = HashMap::new();

        let csv_files: Vec<_> = std::fs::read_dir(dir)
            .map_err(DictError::Io)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            })
            .collect();

        if csv_files.is_empty() {
            return Err(DictError::NoDictFiles(dir.display().to_string()));
        }

        for entry in &csv_files {
            let raw_bytes = std::fs::read(entry.path()).map_err(DictError::Io)?;
            let (utf8, _, _) = EUC_JP.decode(&raw_bytes);
            let cursor = std::io::Cursor::new(utf8.as_bytes());
            let reader = std::io::BufReader::new(cursor);
            let partial = Self::load_from_reader(reader)?;
            for (reading, entries) in partial.entries {
                all_entries.entry(reading).or_default().extend(entries);
            }
        }

        // Sort each entry list by cost ascending
        for entries in all_entries.values_mut() {
            entries.sort_by_key(|e| e.cost);
        }

        Ok(Dictionary {
            entries: all_entries,
        })
    }

    /// Load dictionary from a single IPAdic CSV reader (UTF-8).
    ///
    /// IPAdic CSV format: `surface,left_id,right_id,cost,pos1,pos2,pos3,pos4,conj_type,conj_form,base,reading,pronunciation`
    pub fn load_from_reader(reader: impl BufRead) -> Result<Self, DictError> {
        let mut entries: HashMap<String, Vec<DictEntry>> = HashMap::new();

        for (i, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(DictError::Io)?;
            if line.is_empty() {
                continue;
            }

            let fields: Vec<&str> = parse_csv_line(&line);
            if fields.len() < 13 {
                // Skip malformed lines rather than failing
                continue;
            }

            let surface = fields[0].to_string();
            let left_id: u16 = fields[1].parse().map_err(|_| DictError::Parse {
                line: i + 1,
                reason: format!("invalid left_id: '{}'", fields[1]),
            })?;
            let right_id: u16 = fields[2].parse().map_err(|_| DictError::Parse {
                line: i + 1,
                reason: format!("invalid right_id: '{}'", fields[2]),
            })?;
            let cost: i32 = fields[3].parse().map_err(|_| DictError::Parse {
                line: i + 1,
                reason: format!("invalid cost: '{}'", fields[3]),
            })?;

            let reading_katakana = fields[11];
            let reading = katakana_to_hiragana(reading_katakana);

            if reading.is_empty() {
                continue;
            }

            let entry = DictEntry {
                surface,
                reading: reading.clone(),
                left_id,
                right_id,
                cost,
            };

            entries.entry(reading).or_default().push(entry);
        }

        // Sort each entry list by cost ascending
        for v in entries.values_mut() {
            v.sort_by_key(|e| e.cost);
        }

        Ok(Dictionary { entries })
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
        let suffix = &input[start..];
        let mut results = Vec::new();

        let mut end = start;
        for ch in suffix.chars() {
            end += ch.len_utf8();
            let prefix = &input[start..end];
            if let Some(entries) = self.entries.get(prefix) {
                results.push((end, entries.as_slice()));
            }
        }

        results
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

/// Parse an IPAdic CSV line, handling quoted fields.
///
/// IPAdic uses commas as delimiters and quotes fields containing commas.
fn parse_csv_line(line: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let bytes = line.as_bytes();

    for i in 0..bytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
        } else if bytes[i] == b',' && !in_quotes {
            let field = &line[start..i];
            // Strip surrounding quotes
            let field = field.strip_prefix('"').unwrap_or(field);
            let field = field.strip_suffix('"').unwrap_or(field);
            fields.push(field);
            start = i + 1;
        }
    }
    // Last field
    let field = &line[start..];
    let field = field.strip_prefix('"').unwrap_or(field);
    let field = field.strip_suffix('"').unwrap_or(field);
    fields.push(field);

    fields
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

    #[test]
    fn test_load_from_reader_simple_csv() {
        // IPAdic CSV line (UTF-8, 13 fields):
        // surface,left_id,right_id,cost,pos1,pos2,pos3,pos4,conj_type,conj_form,base,reading,pronunciation
        let csv = "今日,1,2,5000,名詞,一般,*,*,*,*,今日,キョウ,キョー\n\
                   京,3,4,7000,名詞,固有名詞,*,*,*,*,京,キョウ,キョー\n\
                   東京,5,6,3000,名詞,固有名詞,*,*,*,*,東京,トウキョウ,トーキョー\n";
        let reader = std::io::BufReader::new(csv.as_bytes());
        let dict = Dictionary::load_from_reader(reader).unwrap();

        assert_eq!(dict.lookup("きょう").len(), 2);
        // Sorted by cost: 今日(5000) before 京(7000)
        assert_eq!(dict.lookup("きょう")[0].surface, "今日");
        assert_eq!(dict.lookup("きょう")[1].surface, "京");

        assert_eq!(dict.lookup("とうきょう").len(), 1);
        assert_eq!(dict.lookup("とうきょう")[0].surface, "東京");
    }

    #[test]
    fn test_common_prefix_search() {
        let mut entries = HashMap::new();
        entries.insert(
            "き".into(),
            vec![DictEntry {
                surface: "木".into(),
                reading: "き".into(),
                left_id: 1,
                right_id: 1,
                cost: 5000,
            }],
        );
        entries.insert(
            "きょう".into(),
            vec![DictEntry {
                surface: "今日".into(),
                reading: "きょう".into(),
                left_id: 1,
                right_id: 1,
                cost: 3000,
            }],
        );
        let dict = Dictionary { entries };

        let results = dict.common_prefix_search("きょうは", 0);
        assert_eq!(results.len(), 2);
        // First: "き" (shortest prefix)
        assert_eq!(results[0].1[0].surface, "木");
        // Second: "きょう"
        assert_eq!(results[1].1[0].surface, "今日");
    }

    #[test]
    fn test_common_prefix_search_with_offset() {
        let mut entries = HashMap::new();
        entries.insert(
            "は".into(),
            vec![DictEntry {
                surface: "は".into(),
                reading: "は".into(),
                left_id: 1,
                right_id: 1,
                cost: 4000,
            }],
        );
        let dict = Dictionary { entries };

        let input = "きょうは";
        let start = "きょう".len();
        let results = dict.common_prefix_search(input, start);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1[0].surface, "は");
    }

    #[test]
    fn test_connection_cost_from_reader() {
        let data = "3 3\n0 0 -100\n0 1 200\n1 0 50\n2 2 -300\n";
        let reader = std::io::BufReader::new(data.as_bytes());
        let conn = ConnectionCost::from_reader(reader).unwrap();

        assert_eq!(conn.cost(0, 0), -100);
        assert_eq!(conn.cost(0, 1), 200);
        assert_eq!(conn.cost(1, 0), 50);
        assert_eq!(conn.cost(2, 2), -300);
        // Unset entries default to 0
        assert_eq!(conn.cost(1, 1), 0);
    }

    #[test]
    fn test_parse_csv_line_simple() {
        let fields = parse_csv_line("今日,1,2,5000,名詞,一般,*,*,*,*,今日,キョウ,キョー");
        assert_eq!(fields.len(), 13);
        assert_eq!(fields[0], "今日");
        assert_eq!(fields[11], "キョウ");
    }

    #[test]
    fn test_parse_csv_line_quoted() {
        let fields = parse_csv_line("\"hello, world\",1,2");
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], "hello, world");
    }
}
