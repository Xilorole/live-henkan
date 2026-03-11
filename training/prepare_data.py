#!/usr/bin/env python3
"""Prepare training data from Japanese Wikipedia for character-level LM.

Downloads Wikipedia articles via Hugging Face datasets, extracts clean
sentences, and writes them as plain text (one sentence per line).

Usage:
    python prepare_data.py --output data/ [--max-articles 100000]
"""

import argparse
import os
import re
import unicodedata
from pathlib import Path

from datasets import load_dataset
from tqdm import tqdm


def is_japanese_char(c: str) -> bool:
    """Check if a character is CJK, hiragana, or katakana."""
    cp = ord(c)
    return (
        (0x3040 <= cp <= 0x309F)   # Hiragana
        or (0x30A0 <= cp <= 0x30FF)  # Katakana
        or (0x4E00 <= cp <= 0x9FFF)  # CJK Unified
        or (0x3400 <= cp <= 0x4DBF)  # CJK Extension A
        or (0xFF01 <= cp <= 0xFF5E)  # Fullwidth ASCII
        or (0x3000 <= cp <= 0x303F)  # CJK Symbols
    )


def clean_text(text: str) -> str:
    """Clean Wikipedia article text."""
    # Remove references, templates, tags
    text = re.sub(r'\[\[(?:[^|\]]*\|)?([^\]]*)\]\]', r'\1', text)
    text = re.sub(r'\{\{[^}]*\}\}', '', text)
    text = re.sub(r'<[^>]+>', '', text)
    text = re.sub(r'\([^)]*\)', '', text)
    text = re.sub(r'（[^）]*）', '', text)
    # Normalize whitespace
    text = re.sub(r'\s+', ' ', text).strip()
    return text


def extract_sentences(text: str) -> list[str]:
    """Split text into sentences on Japanese punctuation."""
    text = clean_text(text)
    # Split on 。！？ and newlines
    raw = re.split(r'[。！？\n]+', text)
    sentences = []
    for s in raw:
        s = s.strip()
        if len(s) < 5 or len(s) > 200:
            continue
        # Must contain at least some Japanese characters
        jp_count = sum(1 for c in s if is_japanese_char(c))
        if jp_count < len(s) * 0.3:
            continue
        # Normalize to NFKC
        s = unicodedata.normalize('NFKC', s)
        sentences.append(s)
    return sentences


def main():
    parser = argparse.ArgumentParser(description='Prepare LM training data')
    parser.add_argument('--output', type=str, default='data/',
                        help='Output directory')
    parser.add_argument('--max-articles', type=int, default=200000,
                        help='Maximum articles to process')
    parser.add_argument('--val-ratio', type=float, default=0.02,
                        help='Fraction of data for validation')
    args = parser.parse_args()

    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    print("Loading Japanese Wikipedia dataset...")
    dataset = load_dataset(
        "wikipedia", "20220301.ja",
        split="train",
        trust_remote_code=True,
    )

    all_sentences = []
    for i, article in enumerate(tqdm(dataset, desc="Processing articles",
                                     total=min(args.max_articles, len(dataset)))):
        if i >= args.max_articles:
            break
        sentences = extract_sentences(article['text'])
        all_sentences.extend(sentences)

    print(f"Extracted {len(all_sentences)} sentences")

    # Shuffle and split
    import random
    random.seed(42)
    random.shuffle(all_sentences)

    val_size = int(len(all_sentences) * args.val_ratio)
    val_sentences = all_sentences[:val_size]
    train_sentences = all_sentences[val_size:]

    # Write output
    train_path = output_dir / 'train.txt'
    val_path = output_dir / 'val.txt'

    with open(train_path, 'w', encoding='utf-8') as f:
        for s in train_sentences:
            f.write(s + '\n')

    with open(val_path, 'w', encoding='utf-8') as f:
        for s in val_sentences:
            f.write(s + '\n')

    print(f"Train: {len(train_sentences)} sentences -> {train_path}")
    print(f"Val:   {len(val_sentences)} sentences -> {val_path}")

    # Build character vocabulary
    char_set = set()
    for s in all_sentences:
        char_set.update(s)

    # Sort for deterministic ordering
    vocab = sorted(char_set)
    # Add special tokens
    special = ['<pad>', '<unk>', '<bos>', '<eos>']
    full_vocab = special + vocab

    vocab_path = output_dir / 'vocab.txt'
    with open(vocab_path, 'w', encoding='utf-8') as f:
        for c in full_vocab:
            f.write(c + '\n')

    print(f"Vocabulary: {len(full_vocab)} characters -> {vocab_path}")


if __name__ == '__main__':
    main()
