#!/usr/bin/env python3
"""Export trained model to safetensors for Rust candle inference.

Converts PyTorch checkpoint → safetensors format with flat weight names
matching the candle model structure.

Usage:
    python export.py --checkpoint model/best.pt --output ../data/model/
"""

import argparse
import json
import shutil
from collections import OrderedDict
from pathlib import Path

import torch
from safetensors.torch import save_file

from model import CharLM, LMConfig


def flatten_state_dict(state_dict: dict) -> OrderedDict:
    """Rename PyTorch state dict keys to flat candle-compatible names.

    PyTorch: blocks.0.attn.qkv.weight
    Candle:  blocks.0.attn.qkv.weight  (same, candle handles nested modules)
    """
    flat = OrderedDict()
    for key, tensor in state_dict.items():
        # Convert to float32 for safetensors compatibility
        flat[key] = tensor.float().contiguous()
    return flat


def main():
    parser = argparse.ArgumentParser(description='Export model to safetensors')
    parser.add_argument('--checkpoint', type=str, required=True,
                        help='Path to PyTorch checkpoint (best.pt)')
    parser.add_argument('--output', type=str, default='../data/model/',
                        help='Output directory for safetensors + config')
    args = parser.parse_args()

    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Loading checkpoint: {args.checkpoint}")
    ckpt = torch.load(args.checkpoint, map_location='cpu', weights_only=False)

    config = LMConfig(**ckpt['config'])
    model = CharLM(config)
    model.load_state_dict(ckpt['model_state_dict'])
    model.eval()

    print(f"Model: {sum(p.numel() for p in model.parameters()) / 1e6:.2f}M params")
    print(f"Config: d_model={config.d_model}, n_layer={config.n_layer}, "
          f"n_head={config.n_head}, vocab_size={config.vocab_size}")

    # Export weights
    state = flatten_state_dict(model.state_dict())
    safetensors_path = output_dir / 'model.safetensors'
    save_file(state, str(safetensors_path))
    print(f"Saved: {safetensors_path} ({safetensors_path.stat().st_size / 1e6:.2f} MB)")

    # Export config as JSON (for Rust to read)
    config_path = output_dir / 'config.json'
    config.save(str(config_path))
    print(f"Saved: {config_path}")

    # Copy vocab file
    ckpt_dir = Path(args.checkpoint).parent
    # Look for vocab in the data directory (same level as model dir)
    for candidate in [ckpt_dir / 'vocab.txt',
                      ckpt_dir.parent / 'data' / 'vocab.txt',
                      Path('data/vocab.txt')]:
        if candidate.exists():
            shutil.copy(candidate, output_dir / 'vocab.txt')
            print(f"Copied vocab: {candidate} -> {output_dir / 'vocab.txt'}")
            break
    else:
        print("WARNING: vocab.txt not found, copy manually to output dir")

    print(f"\nExport complete. Files in {output_dir}/:")
    for p in sorted(output_dir.iterdir()):
        print(f"  {p.name} ({p.stat().st_size / 1024:.1f} KB)")


if __name__ == '__main__':
    main()
