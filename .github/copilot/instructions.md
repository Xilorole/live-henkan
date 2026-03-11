# Copilot Instructions for live-henkan

## Context

You are working on `live-henkan`, a Rust-based Japanese live-conversion IME.
Read `CLAUDE.md` at the repo root for full architecture and conventions.

## Rules

- Always run `cargo fmt` and `cargo clippy -- -D warnings` before committing
- Write `///` doc comments on all `pub` items
- Use concrete types (structs/enums), never raw `HashMap` for public API return types
- Error types must use `thiserror::Error` derive
- Test names follow: `test_<function>_<scenario>_<expected>`
- Commit messages: `<type>(<scope>): <description>` (e.g., `feat(romaji): add trie lookup`)

## Crate-Specific Guidance

### romaji
- Core type: `RomajiConverter` with `fn feed(&mut self, ch: char) -> RomajiEvent`
- `RomajiEvent`: `Confirmed(String)`, `Pending(String)`, `Invalid`
- Handle `n` ambiguity: `n` + vowel → な行, `n` + consonant/end → ん
- Romaji table is a compile-time Trie built from `ROMAJI_TABLE` const

### dictionary
- Parse mozc-style TSV: reading, surface, POS ID, cost
- `Dictionary::lookup(reading: &str) -> &[DictEntry]`
- `Dictionary::common_prefix_search(input: &str, start: usize) -> Vec<(usize, &[DictEntry])>`
- Use `build.rs` to compile dictionary to binary format

### converter
- `Lattice::build(input: &str, dict: &Dictionary) -> Lattice`
- `Lattice::find_best_path(&self) -> Vec<Segment>`
- Viterbi: forward pass accumulates min cost, backward pass recovers path
- Always insert single-character unknown-word edges as fallback

### engine
- `LiveEngine::new(dict: Dictionary) -> Self`
- `LiveEngine::on_key(&mut self, ch: char) -> EngineOutput`
- `EngineOutput`: `{ committed: String, composing: String, raw: String }`

## When Creating New Files

1. Add the module to `lib.rs` / `main.rs`
2. Add crate dependency in workspace `Cargo.toml` if needed
3. Include at least one test
