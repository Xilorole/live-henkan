# CLAUDE.md — Agent Instructions for live-henkan

## Project Overview

live-henkan is a live-conversion Japanese IME (Input Method Editor) written in Rust.
It converts romaji keystrokes into kanji-kana mixed text in real-time, without requiring
the user to press a conversion key — similar to macOS's "Live Conversion" feature.

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Platform Layer (tsf-frontend / tui-prototype)  │
├─────────────────────────────────────────────────┤
│  Engine (engine crate) — orchestrates below     │
├──────────┬──────────────┬───────────────────────┤
│  romaji  │  dictionary  │  converter            │
│  crate   │  crate       │  crate (lattice+viterbi) │
└──────────┴──────────────┴───────────────────────┘
```

### Crate Responsibilities

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `romaji` | Romaji → Hiragana state machine | `RomajiConverter`, `RomajiEvent` |
| `dictionary` | Dictionary loading & lookup | `Dictionary`, `DictEntry` |
| `converter` | Lattice construction + Viterbi | `Lattice`, `Segment`, `Converter` |
| `engine` | Integrates above into live conversion | `LiveEngine`, `EngineOutput` |
| `tui-prototype` | TUI for development/testing | (binary) |
| `tsf-frontend` | Windows TSF IME frontend | (binary, future) |

## Development Environment

- **Primary**: WSL2 (Ubuntu) with VS Code + GitHub Copilot
- **Target OS**: Windows (TSF) — tested via WSL first, then native Windows build
- **Toolchain**: Rust stable, cargo workspace
- **CI**: GitHub Actions (cargo check, test, clippy, fmt)

## Coding Conventions

- All public APIs must have doc comments (`///`)
- Use `thiserror` for error types, never `Box<dyn Error>` in library crates
- Prefer returning concrete types (`struct`, `enum`) over `HashMap` or tuples
- Tests go in the same file under `#[cfg(test)]` for unit tests, `tests/` for integration
- Commit messages follow Conventional Commits: `feat(romaji): add trie-based state machine`
- One commit = one logical change. Never mix refactoring with feature work.

## Git Workflow

1. All work happens on feature branches: `feat/<crate>-<description>`
2. Open a PR against `main` with the PR template
3. CI must pass (check, test, clippy, fmt) before merge
4. Squash-merge PRs to keep main history clean

## Task Execution Pattern

When working on a task (GitHub Issue):

1. Read the issue description and acceptance criteria
2. Create a feature branch: `git checkout -b feat/<scope>-<short-description>`
3. Write/update types and trait signatures first
4. Write tests that express the acceptance criteria
5. Implement until tests pass
6. Run `cargo fmt && cargo clippy -- -D warnings && cargo test`
7. Commit with conventional commit message
8. Open PR referencing the issue: `Closes #<number>`

## Dictionary Data

Dictionary files are NOT committed to the repo. They are downloaded by `scripts/setup-dict.sh`.
The build script (`crates/dictionary/build.rs`) compiles text dictionaries to binary format.

## Key Design Decisions

- **Trie for romaji**: O(1) per character lookup, naturally handles prefix ambiguity
- **Lattice + Viterbi for conversion**: Standard approach used by mozc, ibus-anthy, etc.
- **Workspace separation**: Each crate is independently testable, easy to scope for AI agents
- **TUI first**: Validate conversion quality before investing in platform IME integration
