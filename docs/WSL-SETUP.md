# WSL環境構築ガイド

## 推奨: Dev Container

Docker Desktop (WSL2バックエンド) がインストール済みなら、VS Codeで
"Reopen in Container" するだけでRust toolchain・just・gh CLI・辞書データが
すべて自動セットアップされる。ホスト環境を汚さず、CI環境との一貫性も保てる。

```bash
# 前提: Docker Desktop + VS Code Dev Containers拡張
git clone https://github.com/<your-username>/live-henkan.git
code live-henkan/
# → "Reopen in Container" を承認
# → post-create.sh が自動実行される
# → gh auth login を手動で1回だけ実行
```

cargo registry と target ディレクトリは Docker volume にマウントされるため、
コンテナを再作成してもキャッシュが残り、再ビルドが速い。

---

## 代替: 手動セットアップ

Docker を使わない場合、以下の手順でWSL上に直接環境を構築する。

## 前提条件

- Windows 11 + WSL2 (Ubuntu)
- VS Code + Remote WSL拡張
- GitHub Copilot 拡張（Agent Mode有効）

## Step 1: Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup default stable
rustup component add rustfmt clippy
```

## Step 2: 開発ツール

```bash
# just (task runner)
cargo install just

# gh CLI (GitHub operations)
sudo apt update && sudo apt install -y gh
gh auth login

# cargo-watch (optional: auto-rebuild on save)
cargo install cargo-watch
```

## Step 3: リポジトリの初期化

```bash
cd ~/projects  # or your preferred location
cp -r /path/to/live-henkan .  # このスキャフォールドをコピー
cd live-henkan

git init
git add .
git commit -m "chore: initial project scaffolding"

# GitHub リポジトリ作成 & push
gh repo create live-henkan --private --source=. --push
```

## Step 4: 辞書データのセットアップ

```bash
./scripts/setup-dict.sh
```

## Step 5: GitHub Issues の作成

```bash
./scripts/create-issues.sh
```

## Step 6: ビルド確認

```bash
just check
# または
cargo build --workspace
cargo test --workspace
```

注: 初回は `todo!()` マクロによりテストが panic するのが正常。
Issue #1 (romaji) の実装から着手する。

## Step 7: VS Code で開発開始

```bash
code .
```

VS Code が開いたら:
1. Rust Analyzer 拡張が自動でワークスペースを認識
2. Copilot Chat を開いて Agent Mode に切り替え
3. 最初のタスクを指示:

```
@workspace Read CLAUDE.md and implement Issue #1: the romaji crate's Trie-based state machine.
Create a feature branch, implement with all tests passing, and commit.
```

## Windows ネイティブビルド（M7以降）

TSFフロントエンドはWindows上でビルドする必要がある:

```powershell
# PowerShell (Windows側)
rustup default stable
cargo build -p tsf-frontend --target x86_64-pc-windows-msvc
```

WSLからWindows側のcargo を呼ぶことも可能:

```bash
# WSL内からcross-compile (要 cross or cargo-xwin)
cargo install cargo-xwin
cargo xwin build -p tsf-frontend --target x86_64-pc-windows-msvc
```
