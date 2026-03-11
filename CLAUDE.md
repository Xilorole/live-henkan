# CLAUDE.md — Agent Instructions for live-henkan

## Project Overview

live-henkan is a live-conversion Japanese IME written in Rust.
It converts romaji keystrokes into kanji-kana mixed text in real-time,
without requiring the user to press a conversion key — similar to macOS's
"Live Conversion" feature.

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Platform Layer (tsf-frontend / tui-prototype)  │
├─────────────────────────────────────────────────┤
│  Engine (engine crate) — orchestrates below     │
├──────────┬──────────────┬──────────┬────────────┤
│  romaji  │  dictionary  │ converter│  scorer     │
│(wana_kana│ (IPAdic CSV) │ (lattice │ (neural LM  │
│ wrapper) │              │+ viterbi)│ re-ranking) │
└──────────┴──────────────┴──────────┴────────────┘
```

### Crate Responsibilities

| Crate | Purpose | Key Types | External Deps |
|-------|---------|-----------|---------------|
| `romaji` | Romaji → Hiragana (incremental) | `IncrementalRomaji`, `RomajiOutput` | `wana_kana` |
| `dictionary` | IPAdic loading & reading-based lookup | `Dictionary`, `DictEntry`, `ConnectionCost` | `encoding_rs` |
| `converter` | Lattice construction + Viterbi + N-best | `Lattice`, `Segment` | `dictionary` |
| `scorer` | Neural LM inference for re-ranking | `LMScorer`, `ScorerError` | `llama-cpp-2`, `hf-hub`, `tokenizers` |
| `engine` | Integrates above into live conversion | `LiveEngine`, `EngineOutput` | all above |
| `tui-prototype` | TUI for development/testing | (binary) | `ratatui`, `crossterm` |
| `tsf-frontend` | Windows TSF IME frontend | (binary, future) | `windows-rs` |

## CRITICAL: Reuse-First Policy

**ALWAYS search for an existing, well-maintained crate before implementing anything.**

Decision framework:
1. Search crates.io and lib.rs for existing solutions
2. Evaluate: downloads, last update, API fit, license compatibility (MIT/Apache-2.0)
3. If a good crate exists → use it, even if its API needs a thin wrapper
4. If only unmaintained/low-quality crates exist → implement yourself
5. If implementing: keep scope minimal, write thorough tests

Current decisions (see `docs/CRATE-SURVEY.md` for full rationale):

| Need | Decision | Reason |
|------|----------|--------|
| Romaji → Hiragana | **Use `wana_kana`** | 1000 words/ms, well-tested, handles all edge cases |
| Dictionary parsing | **Self-implement** | Need reading→surface reverse index (not what lindera/vibrato provide) |
| Lattice + Viterbi | **Self-implement** | Core algorithm, kana→kanji direction differs from standard tokenizers |
| N-best re-ranking | **Self-implement + `llama-cpp-2`** | Lattice N-best + jinen LM scoring via llama.cpp |
| Neural LM inference | **Use `llama-cpp-2` + jinen model** | Pre-trained 26M param GPT-2, GGUF format, auto-downloaded from HF |
| Connection costs | **Use IPAdic matrix.def** | Standard format, just parse it |
| TUI | **Use `ratatui` + `crossterm`** | De facto standard |
| Windows TSF | **Use `windows-rs`** | Official Microsoft crate; reference `ime-rs` and `windows-chewing-tsf` |
| Katakana ↔ Hiragana | **Use `wana_kana`** | Already a dependency |

### Why Not lindera/vibrato for Conversion?

Morphological analyzers tokenize **kanji text** by matching surface forms.
An IME needs to convert **hiragana** to kanji by matching readings.
The lattice construction is fundamentally different:

- Analyzer: input "今日は" → match surface "今日" in dictionary
- IME: input "きょうは" → match reading "きょう" → surface "今日"

We reuse IPAdic **data** (entries + connection costs) but build our own
reading-indexed lookup and lattice construction.

## Development Environment

- **Primary**: VS Code Dev Container (`.devcontainer/`)
- **Alternative**: WSL2 (Ubuntu) — see `docs/WSL-SETUP.md`
- **Target OS**: Windows (TSF) — validated via TUI first, then native Windows build
- **Toolchain**: Rust stable, cargo workspace
- **CI**: GitHub Actions (check, test, clippy, fmt)

## Coding Conventions

- All public APIs must have doc comments (`///`)
- Use `thiserror` for error types, never `Box<dyn Error>` in library crates
- Prefer concrete types (`struct`, `enum`) over `HashMap` or tuples for public APIs
- Tests go in the same file under `#[cfg(test)]` for unit tests, `tests/` for integration
- Commit messages follow Conventional Commits: `feat(romaji): add incremental wrapper`
- One commit = one logical change. Never mix refactoring with feature work.
- Before adding a new dependency, check if an existing dep already covers the need.

## Git Workflow

1. All work happens on feature branches: `feat/<crate>-<description>`
2. Open a PR against `main` with the PR template
3. CI must pass (check, test, clippy, fmt) before merge
4. Squash-merge PRs to keep main history clean

## Task Execution Pattern

When working on a task (GitHub Issue):

1. Read the issue description and acceptance criteria
2. **Search crates.io for existing solutions first** — do not reimplement
3. Create a feature branch: `git checkout -b feat/<scope>-<short-description>`
4. Write/update types and trait signatures first
5. Write tests that express the acceptance criteria
6. Implement until tests pass
7. Run `cargo fmt && cargo clippy -- -D warnings && cargo test`
8. Commit with conventional commit message
9. Open PR referencing the issue: `Closes #<number>`

## Dictionary Data

IPAdic CSV files are NOT committed to the repo.
Downloaded by `scripts/setup-dict.sh` into `data/dictionary/`.
Connection cost matrix (`matrix.def`) is also downloaded.

## Key Design Decisions

- **wana_kana for romaji**: Battle-tested conversion; thin incremental wrapper (~100 lines)
- **IPAdic for dictionary data**: Standard, freely available, includes connection costs
- **Reading-indexed lookup**: Custom reverse index (reading → surface) for kana→kanji
- **Lattice + Viterbi for conversion**: Standard approach; unigram first, bigram later
- **N-best + Neural LM re-ranking**: Viterbi generates top-K paths, jinen LM
  (GPT-2 26M, via llama.cpp) re-ranks by perplexity. Uses pre-trained model
  from karukan project — no training pipeline needed.
- **llama-cpp-2 for inference**: llama.cpp bindings, GGUF format, auto-downloads from HuggingFace
- **Workspace separation**: Each crate is independently testable, easy to scope for AI agents
- **TUI first**: Validate conversion quality before investing in platform IME integration

### Conversion Pipeline

```
Input: ひらがな列
  ↓
Stage 1: Lattice construction (dictionary common prefix search)
  ↓
Stage 2: N-best Viterbi (top-K paths by bigram cost)
  ↓
Stage 3: Neural LM re-ranking (optional, when model loaded)
  - Score each path's surface text via jinen LM (llama.cpp)
  - Interpolate: α * LM_score + (1-α) * normalized_Viterbi_cost
  ↓
Output: Best path → Vec<Segment>
```

### Training Pipeline (`training/`)

Retained for future custom model training. Currently not required — the
scorer uses the pre-trained jinen model (auto-downloaded from HuggingFace).

```
Wikipedia dump → sentence extraction → character vocab
  ↓
Character-level Transformer LM (PyTorch)
  - 3 layers, 256-dim, 4 heads, ~2M params
  - Next-character prediction
  ↓
Export to safetensors → data/model/
  ↓
candle inference in scorer crate (legacy path)
```

## Reference Projects

- **karukan** (togatoga/karukan): Rust + neural kana-kanji + fcitx5
- **ime-rs** (saschanaz/ime-rs): MS IME sample ported to Rust/TSF
- **windows-chewing-tsf**: Production Rust TSF IME for Chinese
- **azooKey-Windows**: Rust TSF client + Swift conversion server
- **MZ-IMEja**: C++/Rust Windows IME with vibrato integration
