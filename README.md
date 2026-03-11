# live-henkan

Rust製のライブ変換日本語IME。ローマ字入力をリアルタイムに漢字かな混じり文に変換する。
macOSの「ライブ変換」と同等の機能をWindows (TSF) で実現することを目標とする。

## Architecture

```
┌─────────────────────────────────────┐
│  Platform Layer (TSF / TUI)         │
├─────────────────────────────────────┤
│  engine: LiveEngine                 │
├──────────┬───────────┬──────────────┤
│  romaji  │ dictionary│  converter   │
└──────────┴───────────┴──────────────┘
```

| Crate | Role |
|-------|------|
| `romaji` | ローマ字→ひらがな変換ステートマシン |
| `dictionary` | mozc辞書の読み込み・検索 |
| `converter` | ラティス構築＋ビタビアルゴリズム |
| `engine` | 上記を統合したライブ変換エンジン |
| `tui-prototype` | TUIプロトタイプ（開発・検証用） |
| `tsf-frontend` | Windows TSF IMEフロントエンド |

## Getting Started

```bash
# Prerequisites: Rust toolchain (rustup), just (optional)
rustup default stable
cargo install just  # optional, for task runner

# Clone and setup
git clone https://github.com/<your-username>/live-henkan.git
cd live-henkan

# Download dictionary data
./scripts/setup-dict.sh

# Run checks
just check
# or: cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

## Development Workflow

See [CLAUDE.md](CLAUDE.md) for detailed agent/developer instructions.

1. Pick an issue from the issue tracker
2. Create feature branch: `git checkout -b feat/<crate>-<description>`
3. Implement with tests → `just check`
4. Open PR → CI validates → Squash merge

## Milestones

- [x] M0: Repository scaffolding & CI
- [ ] M1: `romaji` — Trie-based romaji→hiragana
- [ ] M2: `dictionary` — mozc dictionary loading & search
- [ ] M3: `converter` — Lattice construction
- [ ] M4: `converter` — Viterbi algorithm
- [ ] M5: `engine` — Live conversion integration
- [ ] M6: `tui-prototype` — Terminal UI for testing
- [ ] M7: `tsf-frontend` — Windows TSF integration

## License

MIT
