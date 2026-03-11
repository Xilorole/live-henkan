# Crate Survey & Build vs. Reuse Decisions

## Layer 1: Romaji → Hiragana

### Candidates

| Crate | Downloads | Last Update | API | Verdict |
|-------|-----------|-------------|-----|---------|
| `wana_kana` | 高 | 2024-10 | Batch (`&str → String`) | **ADOPT** |
| `romaji` | 低 | 2017 | Batch, Rust 2015 edition | 古すぎ |
| `kana-jp` | 低 | 新しいが小規模 | Batch | wana_kanaで十分 |

### Decision: `wana_kana` を採用 + 薄いラッパーのみ自作

`wana_kana` はバッチ変換API（`"kyou".to_hiragana() → "きょう"`）のみ提供。
IMEに必要なインクリメンタル入力（1文字ずつfeed）はAPIとして存在しない。

しかし、自前でTrieステートマシンを実装する必要はない。以下の戦略で十分：

```rust
// ローマ字バッファを蓄積し、wana_kanaに毎回全体を渡す
// 前回の出力との差分で「確定済み」と「未確定」を判定
struct IncrementalRomaji {
    buffer: String,
}

impl IncrementalRomaji {
    fn feed(&mut self, ch: char) -> (String /* confirmed */, String /* pending */) {
        self.buffer.push(ch);
        let converted = wana_kana::to_hiragana::to_hiragana(&self.buffer);
        // converted の末尾がまだローマ字ならそれが pending
        // ...差分ロジック（~50行程度）
    }
}
```

wana_kana は 1ms で1000語変換可能なので、キーストロークごとに全体変換しても性能問題なし。

---

## Layer 2: 辞書 + かな漢字変換（ラティス + ビタビ）

### 形態素解析エンジン（使えるか？）

| Crate | Stars | Downloads | 特徴 |
|-------|-------|-----------|------|
| `lindera` | 606 | 919k | IPAdic/UniDic内蔵可、フル機能 |
| `vibrato` | ~200 | 57k | MeCab再実装、最速、辞書学習対応 |
| `vaporetto` | 252 | 181k | ポイント予測型、異なるアプローチ |

### 重要な判断: 形態素解析器 ≠ かな漢字変換器

lindera/vibrato は**表層形**（漢字）でマッチする形態素解析器。
IMEが必要なのは**読み**（ひらがな）→ 表層形（漢字）の逆引き変換。

例：
- 形態素解析: "今日は良い天気" → ["今日", "は", "良い", "天気"] （入力が漢字）
- かな漢字変換: "きょうはいいてんき" → "今日はいい天気" （入力がひらがな）

lindera/vibrato はそのままでは使えない。ただし以下は流用可能：

1. **辞書データ（IPAdic）**: 読み→表層形のマッピング元
2. **連接コスト行列（matrix.def）**: ビタビの品詞バイグラムコスト
3. **未知語処理定義（unk.def, char.def）**: 未知語のコスト設定

### Decision: 辞書データはIPAdic流用、変換エンジンは自作

自作が必要な理由：
- 読み（ひらがな）をキーにした Common Prefix Search が必要
- ラティス構築ロジックが形態素解析と異なる
- ここがこのプロジェクトの **コア価値** であり、外部依存にすべきでない

自作する範囲（実質 ~300-500行）：
- `ReadingIndex`: IPAdic辞書を読み→表層形で逆引きするインデックス
- `Lattice::build()`: ひらがな入力からDAGを構築
- `Lattice::viterbi()`: 最小コスト経路を求める

辞書パースには `lindera-dictionary` の辞書ビルドインフラを活用するか、
IPAdicのCSVを直接パースする。後者のほうが依存が軽い。

---

## Layer 3: TUI プロトタイプ

| Crate | 用途 | Verdict |
|-------|------|---------|
| `ratatui` | TUIフレームワーク | **ADOPT** (デファクト) |
| `crossterm` | ターミナルバックエンド | **ADOPT** (ratatuiの標準組合せ) |

自作不要。

---

## Layer 4: Neural LM Inference

### Candidates

| Crate | Downloads | Last Update | API | Verdict |
|-------|-----------|-------------|-----|---------|
| `candle-core`+`candle-nn` | 高 | Active (HuggingFace) | Tensor ops + nn modules | **ADOPT** |
| `ort` (ONNX Runtime) | 中 | Active | ONNX model loading | C++ dependency, heavy |
| `tch-rs` (LibTorch) | 中 | Active | PyTorch bindings | C++ dependency, 2GB+ |
| `burn` | 中 | Active | Pure Rust ML | API less mature than candle |

### Decision: `candle` を採用

pure-Rust で依存が軽く、safetensors ロードをネイティブサポート。
IME はユーザーの PC で動くので、C++ ランタイム依存は避けたい。
candle は HuggingFace 公式で LLM 推論に実績あり。

---

## Layer 5: Windows TSF

### 参考実装

| Project | 言語 | 特徴 |
|---------|------|------|
| `ime-rs` (saschanaz) | Rust | MS IMEサンプルのRust移植、TSF COM実装の参考 |
| `windows-chewing-tsf` | Rust | 本番稼働している中国語IME、GPL-3.0 |
| `azooKey-Windows` | Rust+Swift | TSF Client (Rust) + 変換Server (Swift) のマルチプロセス構成 |

### Decision: `windows-rs` + 参考実装パターンの踏襲

TSF のCOM実装を抽象化した汎用crateは存在しない。
`windows-rs` で直接 `ITfTextInputProcessor` 等を実装する必要がある。
ただし `ime-rs` と `windows-chewing-tsf` のコードが参考として非常に有用。

---

## Layer 5: 参考にすべき完成品

| Project | アプローチ | 参考価値 |
|---------|-----------|---------|
| karukan (togatoga) | GPT-2/llama.cpp でニューラル変換、Rust、fcitx5 | アーキテクチャ参考 |
| azooKey-Desktop | Swift、ニューラル変換、macOS IMK | ライブ変換のUI/UX参考 |
| mozc (Google) | C++、Viterbi、マルチプラットフォーム | 変換品質のベンチマーク |
| MZ-IMEja | C++/Rust、vibrato統合、Windows IME | TSF + vibrato統合の参考 |

---

## Summary: 最終的なクレート構成

```
live-henkan/
  crates/
    romaji/         → wana_kana のラッパー（~100行）
    dictionary/     → IPAdic CSVパーサー + 読み逆引きインデックス（自作、~300行）
    converter/      → ラティス構築 + ビタビ（自作、~400行）
    engine/         → 統合レイヤー（自作、~200行）
    tui-prototype/  → ratatui + crossterm（自作、~300行）
    tsf-frontend/   → windows-rs + ime-rsパターン（自作、~1000行）
```

外部依存:
- `wana_kana`: ローマ字⇔ひらがな変換
- `ratatui` + `crossterm`: TUI
- `windows-rs`: Windows API バインディング
- `serde` / `serde_json`: 設定ファイル等
- `thiserror`: エラー型

自作する価値があるもの:
- 読み→漢字の逆引きインデックス（形態素解析器とは要件が違う）
- ラティス + ビタビ（コアアルゴリズム、プロジェクトの存在意義）
- TSFグルーコード（抽象化crateが存在しない）
