# Neural LM Training Pipeline for live-henkan

Character-level Transformer Language Model for kana-kanji re-ranking.

## Quick Start

```bash
cd training
pip install -r requirements.txt
python prepare_data.py --output data/
python train.py --data data/train.txt --output model/
python export.py --checkpoint model/best.pt --output ../data/model/
```

## Architecture

- **Model**: Causal Transformer (GPT-like), character-level
- **Size**: ~2M parameters, ~8MB safetensors
- **Task**: Next character prediction on Japanese text
- **Usage**: Score N-best paths from Viterbi by perplexity

## Pipeline

1. `prepare_data.py` — Download & process Japanese Wikipedia
2. `train.py` — Train character-level Transformer LM
3. `export.py` — Export to safetensors for Rust `candle` inference
