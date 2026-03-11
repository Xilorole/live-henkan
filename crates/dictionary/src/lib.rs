//! Dictionary loading and lookup for Japanese IME.
//!
//! Supports loading from mozc-format TSV files and provides
//! exact match and common prefix search.

use thiserror::Error;

/// Errors that can occur during dictionary operations.
#[derive(Debug, Error)]
pub enum DictError {
    #[error("Failed to read dictionary file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse dictionary entry at line {line}: {reason}")]
    Parse { line: usize, reason: String },
}

/// A single dictionary entry mapping a reading to a surface form.
#[derive(Debug, Clone, PartialEq)]
pub struct DictEntry {
    /// Surface form (漢字かな混じり表記).
    pub surface: String,
    /// Reading in hiragana.
    pub reading: String,
    /// Part-of-speech ID.
    pub pos_id: u16,
    /// Cost (lower = more likely).
    pub cost: i32,
}

/// Japanese dictionary providing reading-based lookup.
#[derive(Debug)]
pub struct Dictionary {
    // TODO: Internal storage (HashMap, Trie, or LOUDS)
}

impl Dictionary {
    /// Load a dictionary from a mozc-format TSV file.
    pub fn load_from_tsv(path: &std::path::Path) -> Result<Self, DictError> {
        todo!("Milestone 2: Parse mozc TSV")
    }

    /// Exact match lookup by reading.
    pub fn lookup(&self, reading: &str) -> &[DictEntry] {
        todo!("Milestone 2")
    }

    /// Common prefix search: find all entries whose reading is a prefix of `input[start..]`.
    /// Returns `(end_position, entries)` pairs sorted by length ascending.
    pub fn common_prefix_search(&self, input: &str, start: usize) -> Vec<(usize, Vec<&DictEntry>)> {
        todo!("Milestone 2")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dict_entry_fields() {
        let entry = DictEntry {
            surface: "東京".into(),
            reading: "とうきょう".into(),
            pos_id: 1,
            cost: 5000,
        };
        assert_eq!(entry.surface, "東京");
        assert_eq!(entry.reading, "とうきょう");
    }
}
