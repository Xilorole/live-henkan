#!/usr/bin/env bash
set -euo pipefail

# Create GitHub Issues for all milestones.
# Usage: ./scripts/create-issues.sh
# Requires: gh CLI authenticated with your repo

REPO_FLAG="" # Add "--repo owner/live-henkan" if needed

gh issue create $REPO_FLAG \
  --title "feat(romaji): incremental wana_kana wrapper for IME input" \
  --label "enhancement,milestone:1" \
  --body '## Objective
Implement an incremental romaji→hiragana converter that wraps `wana_kana` for keystroke-by-keystroke IME input.

## Crate
`romaji`

## External Dependencies
- `wana_kana` (already in Cargo.toml) — handles all romaji→hiragana conversion logic

## Acceptance Criteria
- [ ] `IncrementalRomaji::feed(char)` returns `RomajiOutput { confirmed, pending }`
- [ ] Delegates to `wana_kana::to_hiragana()` internally — NO custom Trie or romaji table
- [ ] `flush_pending()` forces conversion of ambiguous trailing input (e.g., lone `n` → `ん`)
- [ ] Non-ASCII characters pass through immediately as confirmed
- [ ] All existing tests in `src/lib.rs` pass
- [ ] `cargo clippy` clean, doc comments on all pub items

## Important
DO NOT implement a custom romaji table or Trie state machine.
The `wana_kana` crate handles all edge cases (digraphs, っ, ん ambiguity, etc.).
The job here is to build an incremental wrapper that diffs batch output.

## Notes
- `split_trailing_romaji()` helper already sketched — splits converted output into leading kana + trailing ASCII
- Performance is not a concern: wana_kana converts ~1000 words/ms
- Type stubs and tests are already in `crates/romaji/src/lib.rs`
'

gh issue create $REPO_FLAG \
  --title "feat(dictionary): IPAdic parser with reading-based reverse index" \
  --label "enhancement,milestone:2" \
  --body '## Objective
Parse mecab-ipadic CSV files and build a reading-indexed dictionary for kana→kanji conversion.

## Crate
`dictionary`

## Acceptance Criteria
- [ ] Parse IPAdic CSV format: `surface,left_id,right_id,cost,pos1,...,reading,pronunciation`
- [ ] Normalize katakana readings to hiragana (IPAdic stores readings in katakana)
- [ ] `Dictionary::lookup(reading)` returns entries sorted by cost ascending
- [ ] `Dictionary::common_prefix_search(input, start)` returns all prefix matches
- [ ] `ConnectionCost::from_reader()` parses `matrix.def` for bigram costs
- [ ] Integration test loading a real IPAdic file subset
- [ ] Handle encoding (IPAdic is EUC-JP by default; use UTF-8 repackaged version)

## Why Self-Implement?
lindera/vibrato are morphological analyzers that match on **surface forms** (漢字).
We need the reverse: match on **readings** (ひらがな) → surface forms (漢字).
See `docs/CRATE-SURVEY.md` for detailed rationale.

## Dependencies
- `scripts/setup-dict.sh` must be run first to download IPAdic files

## Notes
- `katakana_to_hiragana()` helper is already implemented in lib.rs
- For Common Prefix Search: iterate char boundaries from `start`, check each prefix in HashMap
- Consider Double-Array Trie (`yada` or `daachorse` crate) as future optimization if HashMap is too slow
- IPAdic reading field is the second-to-last field in the CSV (index varies by POS)
'

gh issue create $REPO_FLAG \
  --title "feat(converter): lattice construction from hiragana + dictionary" \
  --label "enhancement,milestone:3" \
  --body '## Objective
Build a word lattice (DAG) from a hiragana string using dictionary common prefix search.

## Crate
`converter`

## Acceptance Criteria
- [ ] `Lattice::build(input, dict)` constructs edges for all dictionary matches by reading
- [ ] Unknown-word fallback: single hiragana character edges with high cost (e.g., 10000)
- [ ] Every byte position in the input is reachable (no gaps in the lattice)
- [ ] Test with small inline dictionary: verify correct number of edges

## Dependencies
- Milestone 2 (`dictionary` crate)

## Notes
- `edges[i]` = Vec of edges starting at byte position `i` of the input
- For each char boundary: call `dict.common_prefix_search(input, pos)`, add edges
- Unknown words ensure the lattice is always connected
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
- [ ] Forward pass: compute min cumulative cost to each byte position
- [ ] Backward trace: recover the optimal path
- [ ] Returns `ConvertError::NoPath` if no valid path exists
- [ ] Unigram cost only for first implementation (no connection costs)
- [ ] Test: small dictionary + known input → expected segmentation

## Dependencies
- Milestone 3 (lattice construction)

## Follow-up
- Add bigram connection costs using `ConnectionCost::cost(right_id, left_id)` (separate issue)
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
- [ ] Typing "kyouha" produces composing text with kanji (with dictionary loaded)
- [ ] `commit()` finalizes current composition
- [ ] `reset()` clears all state
- [ ] Works correctly with sequential input (stateful across calls)

## Dependencies
- Milestones 1-4
'

gh issue create $REPO_FLAG \
  --title "feat(tui-prototype): terminal UI for engine testing" \
  --label "enhancement,milestone:6" \
  --body '## Objective
Build a terminal UI using `ratatui` + `crossterm` to interactively test the live conversion engine.

## Crate
`tui-prototype`

## External Dependencies
- `ratatui` (already in Cargo.toml) — DO NOT implement terminal rendering from scratch
- `crossterm` (already in Cargo.toml) — for event handling

## Acceptance Criteria
- [ ] Launches a fullscreen TUI in the terminal
- [ ] Displays: committed text, composing text (underlined/highlighted), pending romaji
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
Implement a Windows TSF (Text Services Framework) frontend for the live-henkan engine.

## Crate
`tsf-frontend`

## External Dependencies
- `windows-rs` (already in Cargo.toml) — DO NOT create a custom COM framework

## Reference Implementations
- `ime-rs` (saschanaz/ime-rs): MS IME sample ported to Rust, excellent TSF reference
- `windows-chewing-tsf` (chewing): Production Rust TSF IME, GPL-3.0 (reference only)
- `azooKey-Windows`: Rust TSF client + Swift server architecture

## Acceptance Criteria
- [ ] Registers as a Windows IME via TSF
- [ ] Receives keystroke input from any Windows application
- [ ] Passes input through LiveEngine
- [ ] Displays composition string inline (underlined)
- [ ] Commits finalized text to the application
- [ ] Can be enabled/disabled via language bar

## Dependencies
- Milestone 5 (engine)
- Milestone 6 (TUI prototype for conversion quality validation)

## Notes
- Implement: ITfTextInputProcessor, ITfKeyEventSink, ITfCompositionSink
- Build & test on native Windows (not WSL)
- Study `ime-rs` COM boilerplate patterns closely before starting
'

echo ""
echo "All 7 issues created successfully."
