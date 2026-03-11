#!/usr/bin/env python3
"""Train character-level Transformer LM on Japanese text.

Usage:
    python train.py --data data/ --output model/ [--epochs 10] [--batch-size 64]
"""

import argparse
import math
import time
from pathlib import Path

import torch
import torch.nn.functional as F
from torch.utils.data import DataLoader, Dataset

from model import CharLM, CharVocab, LMConfig


class TextDataset(Dataset):
    """Line-by-line text dataset with character-level tokenization."""

    def __init__(self, path: str, vocab: CharVocab, max_len: int = 256):
        self.samples = []
        with open(path, 'r', encoding='utf-8') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                ids = vocab.encode(line, add_bos=True, add_eos=True)
                if len(ids) <= max_len:
                    self.samples.append(ids)

    def __len__(self):
        return len(self.samples)

    def __getitem__(self, idx):
        return self.samples[idx]


def collate_fn(batch: list[list[int]]) -> torch.Tensor:
    """Pad sequences to same length in batch."""
    max_len = max(len(s) for s in batch)
    padded = [s + [0] * (max_len - len(s)) for s in batch]
    return torch.tensor(padded, dtype=torch.long)


def evaluate(model: CharLM, dataloader: DataLoader, device: torch.device) -> float:
    """Compute average loss on a dataset."""
    model.eval()
    total_loss = 0.0
    total_tokens = 0
    with torch.no_grad():
        for batch in dataloader:
            batch = batch.to(device)
            logits = model(batch[:, :-1])
            targets = batch[:, 1:]
            # Mask padding
            mask = targets != 0
            loss = F.cross_entropy(
                logits.reshape(-1, logits.size(-1)),
                targets.reshape(-1),
                ignore_index=0,
                reduction='sum'
            )
            total_loss += loss.item()
            total_tokens += mask.sum().item()
    return total_loss / max(total_tokens, 1)


def main():
    parser = argparse.ArgumentParser(description='Train character LM')
    parser.add_argument('--data', type=str, default='data/',
                        help='Data directory (with train.txt, val.txt, vocab.txt)')
    parser.add_argument('--output', type=str, default='model/',
                        help='Output directory for checkpoints')
    parser.add_argument('--epochs', type=int, default=10)
    parser.add_argument('--batch-size', type=int, default=64)
    parser.add_argument('--lr', type=float, default=3e-4)
    parser.add_argument('--d-model', type=int, default=256)
    parser.add_argument('--n-layer', type=int, default=3)
    parser.add_argument('--n-head', type=int, default=4)
    parser.add_argument('--max-seq-len', type=int, default=256)
    args = parser.parse_args()

    data_dir = Path(args.data)
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    device = torch.device('cuda' if torch.cuda.is_available() else
                          'mps' if torch.backends.mps.is_available() else 'cpu')
    print(f"Device: {device}")

    # Load vocab
    vocab = CharVocab(str(data_dir / 'vocab.txt'))
    print(f"Vocabulary size: {vocab.size}")

    # Create model config
    config = LMConfig(
        vocab_size=vocab.size,
        d_model=args.d_model,
        n_head=args.n_head,
        n_layer=args.n_layer,
        d_ff=args.d_model * 2,
        max_seq_len=args.max_seq_len,
    )
    config.save(str(output_dir / 'config.json'))

    model = CharLM(config).to(device)

    # Load data
    print("Loading training data...")
    train_ds = TextDataset(str(data_dir / 'train.txt'), vocab, args.max_seq_len)
    val_ds = TextDataset(str(data_dir / 'val.txt'), vocab, args.max_seq_len)
    print(f"Train: {len(train_ds)} sentences, Val: {len(val_ds)} sentences")

    train_loader = DataLoader(train_ds, batch_size=args.batch_size,
                              shuffle=True, collate_fn=collate_fn,
                              num_workers=2, pin_memory=True)
    val_loader = DataLoader(val_ds, batch_size=args.batch_size,
                            shuffle=False, collate_fn=collate_fn,
                            num_workers=2, pin_memory=True)

    # Optimizer
    optimizer = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=0.01)
    scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(
        optimizer, T_max=args.epochs * len(train_loader)
    )

    best_val_loss = float('inf')

    for epoch in range(1, args.epochs + 1):
        model.train()
        total_loss = 0.0
        total_tokens = 0
        t0 = time.time()

        for batch_idx, batch in enumerate(train_loader):
            batch = batch.to(device)
            logits = model(batch[:, :-1])
            targets = batch[:, 1:]

            loss = F.cross_entropy(
                logits.reshape(-1, logits.size(-1)),
                targets.reshape(-1),
                ignore_index=0,
            )

            optimizer.zero_grad()
            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()
            scheduler.step()

            mask = targets != 0
            total_loss += loss.item() * mask.sum().item()
            total_tokens += mask.sum().item()

            if (batch_idx + 1) % 500 == 0:
                avg = total_loss / total_tokens
                ppl = math.exp(min(avg, 20))
                print(f"  [{batch_idx+1}/{len(train_loader)}] "
                      f"loss={avg:.4f} ppl={ppl:.2f}")

        train_loss = total_loss / max(total_tokens, 1)
        val_loss = evaluate(model, val_loader, device)
        train_ppl = math.exp(min(train_loss, 20))
        val_ppl = math.exp(min(val_loss, 20))
        elapsed = time.time() - t0

        print(f"Epoch {epoch}/{args.epochs} ({elapsed:.0f}s): "
              f"train_loss={train_loss:.4f} ppl={train_ppl:.2f} | "
              f"val_loss={val_loss:.4f} ppl={val_ppl:.2f}")

        # Save checkpoint
        ckpt = {
            'epoch': epoch,
            'model_state_dict': model.state_dict(),
            'optimizer_state_dict': optimizer.state_dict(),
            'val_loss': val_loss,
            'config': config.__dict__,
        }
        torch.save(ckpt, output_dir / 'last.pt')

        if val_loss < best_val_loss:
            best_val_loss = val_loss
            torch.save(ckpt, output_dir / 'best.pt')
            print(f"  -> New best model (val_loss={val_loss:.4f})")

    print(f"\nTraining complete. Best val_loss={best_val_loss:.4f}")
    print(f"Best model: {output_dir / 'best.pt'}")


if __name__ == '__main__':
    main()
