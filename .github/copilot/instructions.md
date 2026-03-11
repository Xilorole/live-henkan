# Copilot Instructions for live-henkan

## Context

You are working on `live-henkan`, a Rust-based Japanese live-conversion IME.
Read `CLAUDE.md` at the repo root for full architecture and conventions.

## CRITICAL: Reuse-First Rule

**Before writing ANY implementation code, search crates.io for an existing crate.**

1. If a well-maintained crate exists → add it as a dependency and wrap if needed
2. If only abandoned/low-quality crates exist → implement yourself with tests
3. Never reimplement what `wana_kana`, `ratatui`, `crossterm`, or `windows-rs` already provide
4. When in doubt, check `docs/CRATE-SURVEY.md` for prior decisions

## Rules

- Always run `cargo fmt` and `cargo clippy -- -D warnings` before committing
- Write `///` doc comments on all `pub` items
- Use concrete types (structs/enums), never raw `HashMap` for public API return types
- Error types must use `thiserror::Error` derive
- Test names follow: `test_<function>_<scenario>_<expected>`
- Commit messages: `<type>(<scope>): <description>` (e.g., `feat(romaji): add incremental wrapper`)

## Crate-Specific Guidance

### romaji (wraps `wana_kana`)
- Core type: `IncrementalRomaji` with `fn feed(&mut self, ch: char) -> RomajiOutput`
- `RomajiOutput`: `{ confirmed: String, pending: String }`
- Delegates to `wana_kana::to_hiragana()` on each keystroke, diffs output
- DO NOT implement a custom Trie or romaji table — wana_kana handles all edge cases

### dictionary (self-implemented, uses IPAdic data)
- Parse IPAdic CSV: `surface,left_id,right_id,cost,pos1,...,reading,pronunciation`
- **Reading field is katakana** — normalize to hiragana with `katakana_to_hiragana()`
- `Dictionary::lookup(reading: &str) -> &[DictEntry]` — exact match by reading
- `Dictionary::common_prefix_search(input: &str, start: usize) -> Vec<(usize, &[DictEntry])>`
- `ConnectionCost::from_reader()` — parse `matrix.def`
- This is NOT a morphological analyzer; it's a **reading → surface** reverse index

### converter (self-implemented)
- `Lattice::build(input: &str, dict: &Dictionary) -> Lattice`
- `Lattice::find_best_path(&self, conn: &ConnectionCost) -> Vec<Segment>`
- Viterbi: forward pass accumulates min cost, backward pass recovers path
- Always insert single-character unknown-word edges as fallback
- Start with unigram cost only, add bigram connection costs as follow-up

### engine
- `LiveEngine::new(dict: Dictionary, conn: ConnectionCost) -> Self`
- `LiveEngine::on_key(&mut self, ch: char) -> EngineOutput`
- `EngineOutput`: `{ committed: String, composing: String, raw_pending: String }`
- Uses `IncrementalRomaji` from romaji crate for input processing

### tui-prototype (uses `ratatui` + `crossterm`)
- DO NOT implement terminal rendering from scratch
- Use `ratatui` widgets; `crossterm` for event handling
- Display: committed text + composing text (underlined) + pending romaji

### tsf-frontend (future, uses `windows-rs`)
- Reference: `ime-rs`, `windows-chewing-tsf`, `azooKey-Windows`
- Implement `ITfTextInputProcessor`, `ITfKeyEventSink`, etc. via `windows-rs`
- DO NOT create a custom COM framework

## When Creating New Files

1. Add the module to `lib.rs` / `main.rs`
2. Add crate dependency in workspace `Cargo.toml` if needed
3. Include at least one test
4. **Check if an existing crate covers the need first**
