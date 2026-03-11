#!/usr/bin/env bash
set -euo pipefail

# Download and prepare dictionary data for live-henkan.
# Run from repo root: ./scripts/setup-dict.sh

DICT_DIR="data/dictionary"
MOZC_REPO="https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss"

mkdir -p "$DICT_DIR"

echo "Downloading mozc dictionary files..."

FILES=(
    "dictionary00.txt"
    "dictionary01.txt"
    "dictionary02.txt"
    "dictionary03.txt"
    "dictionary04.txt"
    "dictionary05.txt"
    "dictionary06.txt"
    "dictionary07.txt"
    "dictionary08.txt"
    "dictionary09.txt"
)

for f in "${FILES[@]}"; do
    if [ ! -f "$DICT_DIR/$f" ]; then
        echo "  Downloading $f ..."
        curl -sL "$MOZC_REPO/$f" -o "$DICT_DIR/$f"
    else
        echo "  $f already exists, skipping."
    fi
done

# Also grab connection cost matrix
MATRIX_URL="https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/connection_single_column.txt"
if [ ! -f "$DICT_DIR/connection.txt" ]; then
    echo "  Downloading connection cost matrix..."
    curl -sL "$MATRIX_URL" -o "$DICT_DIR/connection.txt"
fi

echo ""
echo "Done. Dictionary files are in $DICT_DIR/"
echo "These files are .gitignored and will not be committed."
