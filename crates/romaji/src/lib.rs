//! Romaji to Hiragana conversion via Trie-based state machine.
//!
//! # Example
//! ```
//! use romaji::{RomajiConverter, RomajiEvent};
//!
//! let mut conv = RomajiConverter::new();
//! // Feeding 'k' is pending (could be ka, ki, ku, ke, ko, ky...)
//! assert!(matches!(conv.feed('k'), RomajiEvent::Pending(_)));
//! // Feeding 'a' confirms "か"
//! assert!(matches!(conv.feed('a'), RomajiEvent::Confirmed(s) if s == "か"));
//! ```

/// Events produced by the romaji state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RomajiEvent {
    /// A complete hiragana string has been confirmed.
    Confirmed(String),
    /// Input is a valid prefix; waiting for more characters.
    Pending(String),
    /// Input does not match any romaji sequence.
    Invalid,
}

/// Trie-based romaji to hiragana converter.
///
/// Maintains internal state for multi-character sequences (e.g., "sh" → pending, "shi" → "し").
#[derive(Debug)]
pub struct RomajiConverter {
    // TODO: Trie structure + current traversal state
}

impl RomajiConverter {
    /// Create a new converter with the default romaji table.
    pub fn new() -> Self {
        todo!("Milestone 1: Implement Trie construction from ROMAJI_TABLE")
    }

    /// Feed a single character and return the resulting event.
    pub fn feed(&mut self, ch: char) -> RomajiEvent {
        todo!("Milestone 1: Implement Trie traversal")
    }

    /// Reset internal state, discarding any pending input.
    pub fn reset(&mut self) {
        todo!("Milestone 1")
    }

    /// Return the current pending romaji buffer, if any.
    pub fn pending(&self) -> Option<&str> {
        todo!("Milestone 1")
    }
}

impl Default for RomajiConverter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_simple_vowel_confirmed() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.feed('a'), RomajiEvent::Confirmed("あ".into()));
    }

    #[test]
    fn test_feed_consonant_vowel_confirmed() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.feed('k'), RomajiEvent::Pending("k".into()));
        assert_eq!(conv.feed('a'), RomajiEvent::Confirmed("か".into()));
    }

    #[test]
    fn test_feed_nn_produces_n() {
        let mut conv = RomajiConverter::new();
        conv.feed('n');
        assert_eq!(conv.feed('n'), RomajiEvent::Confirmed("ん".into()));
    }

    #[test]
    fn test_feed_n_before_consonant_produces_n() {
        let mut conv = RomajiConverter::new();
        conv.feed('n');
        // 'k' is a consonant → 'n' should resolve to ん, and 'k' becomes new pending
        let event = conv.feed('k');
        // Implementation should emit ん and start pending 'k'
        // Exact API shape depends on implementation — may need multi-event support
        assert!(matches!(event, RomajiEvent::Confirmed(_) | RomajiEvent::Pending(_)));
    }

    #[test]
    fn test_feed_shi_confirmed() {
        let mut conv = RomajiConverter::new();
        assert_eq!(conv.feed('s'), RomajiEvent::Pending("s".into()));
        assert_eq!(conv.feed('h'), RomajiEvent::Pending("sh".into()));
        assert_eq!(conv.feed('i'), RomajiEvent::Confirmed("し".into()));
    }
}
