//! Live conversion engine integrating romaji, dictionary, and converter.
//!
//! Processes keystroke-by-keystroke input and produces continuously
//! updated conversion output — the core of "live conversion".

use converter::Segment;
use dictionary::Dictionary;
use romaji::{RomajiConverter, RomajiEvent};
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

/// The live conversion engine.
#[derive(Debug)]
pub struct LiveEngine {
    romaji: RomajiConverter,
    dict: Dictionary,
    /// Accumulated hiragana buffer.
    hiragana_buf: String,
    /// Committed output so far.
    committed: String,
}

impl LiveEngine {
    /// Create a new engine with the given dictionary.
    pub fn new(dict: Dictionary) -> Self {
        todo!("Milestone 5")
    }

    /// Process a single key input and return the updated output.
    pub fn on_key(&mut self, ch: char) -> EngineOutput {
        todo!("Milestone 5: feed romaji, accumulate hiragana, run converter, produce output")
    }

    /// Commit the current composition and reset.
    pub fn commit(&mut self) -> String {
        todo!("Milestone 5")
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        todo!("Milestone 5")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_output_structure() {
        let output = EngineOutput {
            committed: String::new(),
            composing: "今日は".into(),
            raw_pending: "".into(),
        };
        assert_eq!(output.composing, "今日は");
    }
}
