#!/usr/bin/env python3
"""Character-level Transformer Language Model.

Small causal Transformer for scoring kana-kanji conversion candidates.
Designed to be lightweight enough for real-time IME re-ranking (~2M params).
"""

import json
import math
from dataclasses import dataclass
from pathlib import Path

import torch
import torch.nn as nn
import torch.nn.functional as F


@dataclass
class LMConfig:
    """Model configuration, serializable to JSON for Rust interop."""
    vocab_size: int = 4096
    d_model: int = 256
    n_head: int = 4
    n_layer: int = 3
    d_ff: int = 512
    max_seq_len: int = 256
    dropout: float = 0.1

    def save(self, path: str):
        with open(path, 'w') as f:
            json.dump(self.__dict__, f, indent=2)

    @classmethod
    def load(cls, path: str) -> 'LMConfig':
        with open(path) as f:
            return cls(**json.load(f))


class CharVocab:
    """Character-level vocabulary with special tokens."""

    SPECIAL = ['<pad>', '<unk>', '<bos>', '<eos>']
    PAD_ID = 0
    UNK_ID = 1
    BOS_ID = 2
    EOS_ID = 3

    def __init__(self, vocab_path: str):
        self.char_to_id = {}
        self.id_to_char = {}
        with open(vocab_path, 'r', encoding='utf-8') as f:
            for i, line in enumerate(f):
                c = line.rstrip('\n')
                self.char_to_id[c] = i
                self.id_to_char[i] = c

    def encode(self, text: str, add_bos: bool = True, add_eos: bool = True) -> list[int]:
        ids = []
        if add_bos:
            ids.append(self.BOS_ID)
        for c in text:
            ids.append(self.char_to_id.get(c, self.UNK_ID))
        if add_eos:
            ids.append(self.EOS_ID)
        return ids

    def decode(self, ids: list[int]) -> str:
        return ''.join(
            self.id_to_char.get(i, '?')
            for i in ids
            if i not in (self.PAD_ID, self.BOS_ID, self.EOS_ID)
        )

    @property
    def size(self) -> int:
        return len(self.char_to_id)


class CausalSelfAttention(nn.Module):
    def __init__(self, config: LMConfig):
        super().__init__()
        assert config.d_model % config.n_head == 0
        self.n_head = config.n_head
        self.d_head = config.d_model // config.n_head
        self.qkv = nn.Linear(config.d_model, 3 * config.d_model)
        self.proj = nn.Linear(config.d_model, config.d_model)
        self.dropout = nn.Dropout(config.dropout)
        self.register_buffer(
            'mask',
            torch.tril(torch.ones(config.max_seq_len, config.max_seq_len))
                 .view(1, 1, config.max_seq_len, config.max_seq_len)
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        B, T, C = x.size()
        q, k, v = self.qkv(x).split(C, dim=2)
        q = q.view(B, T, self.n_head, self.d_head).transpose(1, 2)
        k = k.view(B, T, self.n_head, self.d_head).transpose(1, 2)
        v = v.view(B, T, self.n_head, self.d_head).transpose(1, 2)

        att = (q @ k.transpose(-2, -1)) * (1.0 / math.sqrt(self.d_head))
        att = att.masked_fill(self.mask[:, :, :T, :T] == 0, float('-inf'))
        att = F.softmax(att, dim=-1)
        att = self.dropout(att)
        y = att @ v

        y = y.transpose(1, 2).contiguous().view(B, T, C)
        return self.dropout(self.proj(y))


class TransformerBlock(nn.Module):
    def __init__(self, config: LMConfig):
        super().__init__()
        self.ln1 = nn.LayerNorm(config.d_model)
        self.attn = CausalSelfAttention(config)
        self.ln2 = nn.LayerNorm(config.d_model)
        self.ff = nn.Sequential(
            nn.Linear(config.d_model, config.d_ff),
            nn.GELU(),
            nn.Linear(config.d_ff, config.d_model),
            nn.Dropout(config.dropout),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = x + self.attn(self.ln1(x))
        x = x + self.ff(self.ln2(x))
        return x


class CharLM(nn.Module):
    """Character-level causal language model."""

    def __init__(self, config: LMConfig):
        super().__init__()
        self.config = config
        self.tok_emb = nn.Embedding(config.vocab_size, config.d_model)
        self.pos_emb = nn.Embedding(config.max_seq_len, config.d_model)
        self.drop = nn.Dropout(config.dropout)
        self.blocks = nn.ModuleList(
            [TransformerBlock(config) for _ in range(config.n_layer)]
        )
        self.ln_f = nn.LayerNorm(config.d_model)
        self.head = nn.Linear(config.d_model, config.vocab_size, bias=False)

        # Weight tying
        self.head.weight = self.tok_emb.weight

        self.apply(self._init_weights)
        print(f"CharLM: {sum(p.numel() for p in self.parameters()) / 1e6:.2f}M params")

    @staticmethod
    def _init_weights(module):
        if isinstance(module, (nn.Linear, nn.Embedding)):
            nn.init.normal_(module.weight, mean=0.0, std=0.02)
            if isinstance(module, nn.Linear) and module.bias is not None:
                nn.init.zeros_(module.bias)

    def forward(self, idx: torch.Tensor) -> torch.Tensor:
        """Forward pass. Returns logits (B, T, vocab_size)."""
        B, T = idx.size()
        assert T <= self.config.max_seq_len, f"Sequence too long: {T} > {self.config.max_seq_len}"

        tok = self.tok_emb(idx)
        pos = self.pos_emb(torch.arange(T, device=idx.device))
        x = self.drop(tok + pos)

        for block in self.blocks:
            x = block(x)

        x = self.ln_f(x)
        logits = self.head(x)
        return logits

    def score(self, idx: torch.Tensor) -> float:
        """Compute negative log-likelihood per character for a sequence.

        Lower score = more natural text. Used for re-ranking N-best paths.
        """
        with torch.no_grad():
            logits = self.forward(idx.unsqueeze(0))  # (1, T, V)
            # Shift for next-token prediction
            logits = logits[:, :-1, :].contiguous()
            targets = idx[1:].unsqueeze(0).contiguous()
            loss = F.cross_entropy(
                logits.view(-1, logits.size(-1)),
                targets.view(-1),
                reduction='mean'
            )
            return loss.item()
