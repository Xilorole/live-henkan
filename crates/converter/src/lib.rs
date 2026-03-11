//! Lattice construction and Viterbi algorithm for Japanese conversion.
//!
//! Given a hiragana string and a dictionary, builds a word lattice
//! (DAG) and finds the minimum-cost path through it.

use dictionary::{DictEntry, Dictionary};
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
}

/// An edge in the word lattice.
#[derive(Debug, Clone)]
pub struct LatticeEdge {
    /// End position (byte index in the input string).
    pub end: usize,
    /// The dictionary entry for this edge.
    pub surface: String,
    pub reading: String,
    pub cost: i32,
    pub pos_id: u16,
}

/// Word lattice: a DAG over positions in the input string.
#[derive(Debug)]
pub struct Lattice {
    /// `edges[i]` = edges starting at byte position `i`.
    edges: Vec<Vec<LatticeEdge>>,
    input_len: usize,
}

impl Lattice {
    /// Build a lattice from a hiragana input string using the given dictionary.
    pub fn build(input: &str, dict: &Dictionary) -> Self {
        todo!("Milestone 3: Lattice construction with common prefix search + unknown word fallback")
    }

    /// Find the minimum-cost path through the lattice using the Viterbi algorithm.
    pub fn find_best_path(&self) -> Result<Vec<Segment>, ConvertError> {
        todo!("Milestone 4: Viterbi forward pass + backward trace")
    }
}

/// High-level conversion function.
pub fn convert(input: &str, dict: &Dictionary) -> Result<Vec<Segment>, ConvertError> {
    if input.is_empty() {
        return Err(ConvertError::EmptyInput);
    }
    let lattice = Lattice::build(input, dict);
    lattice.find_best_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_structure() {
        let seg = Segment {
            surface: "今日".into(),
            reading: "きょう".into(),
            cost: 3000,
        };
        assert_eq!(seg.surface, "今日");
    }
}
