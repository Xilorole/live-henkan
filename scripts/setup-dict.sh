#!/usr/bin/env bash
set -euo pipefail

# Download and prepare mecab-ipadic dictionary data for live-henkan.
# Run from repo root: ./scripts/setup-dict.sh

DICT_DIR="data/dictionary"
IPADIC_VERSION="2.7.0-20070801"
IPADIC_URL="https://sourceforge.net/projects/mecab/files/mecab-ipadic/2.7.0-20070801/mecab-ipadic-${IPADIC_VERSION}.tar.gz/download"
IPADIC_DIR="${DICT_DIR}/mecab-ipadic-${IPADIC_VERSION}"

mkdir -p "$DICT_DIR"

if [ -d "$IPADIC_DIR" ]; then
    echo "IPAdic already exists at $IPADIC_DIR, skipping download."
else
    echo "Downloading mecab-ipadic-${IPADIC_VERSION}..."
    TMP_FILE=$(mktemp /tmp/ipadic.XXXXXX.tar.gz)
    curl -sL "$IPADIC_URL" -o "$TMP_FILE"

    echo "Extracting..."
    tar xzf "$TMP_FILE" -C "$DICT_DIR"
    rm "$TMP_FILE"
fi

# Verify key files exist
REQUIRED_FILES=("matrix.def" "char.def" "unk.def" "Noun.csv" "Verb.csv" "Adj.csv")
MISSING=0
for f in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$IPADIC_DIR/$f" ]; then
        echo "WARNING: Missing expected file $IPADIC_DIR/$f"
        MISSING=1
    fi
done

if [ "$MISSING" -eq 0 ]; then
    echo ""
    echo "Done. IPAdic files are in $IPADIC_DIR/"
    echo ""
    echo "Key files:"
    echo "  CSV dictionaries: Noun.csv, Verb.csv, Adj.csv, etc."
    echo "  Connection costs: matrix.def"
    echo "  Unknown word:     unk.def + char.def"
    echo ""
    echo "These files are .gitignored and will not be committed."
else
    echo ""
    echo "WARNING: Some files are missing. Dictionary may be incomplete."
fi
