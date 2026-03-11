#!/usr/bin/env bash
set -euo pipefail

# Create GitHub Issues for all milestones.
# Usage: ./scripts/create-issues.sh
# Requires: gh CLI authenticated with your repo

REPO_FLAG="" # Add "--repo owner/live-henkan" if needed

gh issue create $REPO_FLAG \
  --title "feat(romaji): Trie-based romaji to hiragana state machine" \
  --label "enhancement,milestone:1" \
  --body '## Objective
Implement a Trie-based state machine that converts romaji input to hiragana, one character at a time.

## Crate
`romaji`

## Acceptance Criteria
- [ ] `RomajiConverter::new()` builds a Trie from the standard romaji table
- [ ] `feed(char)` returns `Confirmed(String)` when a complete hiragana is produced
- [ ] `feed(char)` returns `Pending(String)` for valid prefixes
- [ ] `feed(char)` returns `Invalid` for impossible sequences
- [ ] `n` + vowel → な行, `n` + consonant → ん + new pending, `nn` → ん
- [ ] Double consonant (e.g., `kk`) → っ + new pending
- [ ] All tests in `src/lib.rs` pass
- [ ] `cargo clippy` clean, doc comments on all pub items

## API
```rust
pub enum RomajiEvent { Confirmed(String), Pending(String), Invalid }
pub struct RomajiConverter { /* trie + state */ }
impl RomajiConverter {
    pub fn new() -> Self;
    pub fn feed(&mut self, ch: char) -> RomajiEvent;
    pub fn reset(&mut self);
    pub fn pending(&self) -> Option<&str>;
}
```

## Notes
- Romaji table should cover: basic vowels, consonant+vowel, digraphs (sh, ch, ts), y-combos (ky, ny, etc.), double consonants, n-special
- Consider `phf` crate or hand-rolled Trie; hand-rolled is preferred for learning value
'

gh issue create $REPO_FLAG \
  --title "feat(dictionary): mozc dictionary parser and lookup" \
  --label "enhancement,milestone:2" \
  --body '## Objective
Load mozc-format TSV dictionary files and provide exact match + common prefix search.

## Crate
`dictionary`

## Acceptance Criteria
- [ ] Parse mozc TSV format: `reading\t(lid)\t(rid)\tcost\tsurface`
- [ ] `Dictionary::lookup(reading)` returns matching entries sorted by cost
- [ ] `Dictionary::common_prefix_search(input, start)` returns all prefix matches
- [ ] Handle loading errors gracefully with `DictError`
- [ ] Unit tests with small inline test dictionaries
- [ ] Integration test with a sample subset of mozc dictionary

## Dependencies
- `scripts/setup-dict.sh` must be run first to download dictionary files

## Notes
- Start with `HashMap<String, Vec<DictEntry>>` for storage
- Common prefix search: iterate char boundaries from `start`, check each prefix
- Consider a `build.rs` step later for binary compilation (not required for M2)
'

gh issue create $REPO_FLAG \
  --title "feat(converter): lattice construction from hiragana + dictionary" \
  --label "enhancement,milestone:3" \
  --body '## Objective
Build a word lattice (DAG) from a hiragana string using dictionary common prefix search.

## Crate
`converter`

## Acceptance Criteria
- [ ] `Lattice::build(input, dict)` constructs edges for all dictionary matches
- [ ] Unknown-word fallback: single hiragana character edges with high cost
- [ ] Every position in the input is reachable (no gaps in the lattice)
- [ ] Test with small dictionary: verify correct number of edges

## Dependencies
- Milestone 2 (`dictionary` crate)

## Notes
- `edges[i]` = Vec of edges starting at byte position `i` of the input
- Iterate over each char boundary, run common_prefix_search, add edges
- For unknown words: add single-char edge with cost = 10000 (configurable)
'

gh issue create $REPO_FLAG \
  --title "feat(converter): Viterbi algorithm for minimum-cost path" \
  --label "enhancement,milestone:4" \
  --body '## Objective
Implement Viterbi algorithm to find the minimum-cost path through the word lattice.

## Crate
`converter`

## Acceptance Criteria
- [ ] `Lattice::find_best_path()` returns `Vec<Segment>` with minimum total cost
- [ ] Forward pass: compute min cumulative cost to each position
- [ ] Backward trace: recover the optimal path
- [ ] Returns `ConvertError::NoPath` if no valid path exists
- [ ] Test: "きょうはいいてんきです" → reasonable segmentation
- [ ] Test: single-character input works (fallback to unknown word)

## Dependencies
- Milestone 3 (lattice construction)

## Notes
- Start with unigram cost only (no bigram connection cost)
- `best_cost[i]` = minimum cost to reach position `i`
- `best_prev[i]` = (start_position, edge_index) for backtracing
- Bigram connection costs can be added as a follow-up issue
'

gh issue create $REPO_FLAG \
  --title "feat(engine): live conversion engine integrating all crates" \
  --label "enhancement,milestone:5" \
  --body '## Objective
Integrate romaji, dictionary, and converter into a keystroke-driven live conversion engine.

## Crate
`engine`

## Acceptance Criteria
- [ ] `LiveEngine::on_key(char)` processes input and returns `EngineOutput`
- [ ] `EngineOutput` contains: committed text, composing text (live conversion), raw pending romaji
- [ ] Typing "kyouha" produces composing text like "今日は" (with dictionary)
- [ ] `commit()` finalizes current composition
- [ ] `reset()` clears all state
- [ ] Works correctly with sequential input (stateful across calls)

## Dependencies
- Milestones 1-4

## Notes
- On each key: feed to romaji → if confirmed, append to hiragana buffer → run converter → update output
- Deferred commit: only auto-commit early segments when confidence is high (stretch goal)
'

gh issue create $REPO_FLAG \
  --title "feat(tui-prototype): terminal UI for engine testing" \
  --label "enhancement,milestone:6" \
  --body '## Objective
Build a terminal UI using ratatui + crossterm to interactively test the live conversion engine.

## Crate
`tui-prototype`

## Acceptance Criteria
- [ ] Launches a fullscreen TUI in the terminal
- [ ] Displays: committed text, composing text (underlined), pending romaji
- [ ] Key input is processed through LiveEngine in real-time
- [ ] Enter commits current composition
- [ ] Escape resets
- [ ] Ctrl+C exits

## Dependencies
- Milestone 5 (engine)
'

gh issue create $REPO_FLAG \
  --title "feat(tsf-frontend): Windows TSF IME integration" \
  --label "enhancement,milestone:7,platform:windows" \
  --body '## Objective
Implement a Windows TSF (Text Services Framework) frontend that connects the live-henkan engine to Windows applications.

## Crate
`tsf-frontend`

## Acceptance Criteria
- [ ] Registers as a Windows IME via TSF
- [ ] Receives keystroke input from any Windows application
- [ ] Passes input through LiveEngine
- [ ] Displays composition string inline (underlined)
- [ ] Commits finalized text to the application
- [ ] Can be enabled/disabled via language bar

## Dependencies
- Milestone 5 (engine)
- Milestone 6 (TUI prototype for validation)

## Notes
- TSF requires COM interop — consider `windows-rs` crate
- Reference: https://docs.microsoft.com/en-us/windows/win32/tsf/text-services-framework
- May need to implement: ITfTextInputProcessor, ITfKeyEventSink, ITfCompositionSink
- Build & test on native Windows (not WSL)
'

echo ""
echo "All issues created successfully."
