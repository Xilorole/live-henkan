#!/usr/bin/env bash
set -euo pipefail

echo "=== live-henkan: post-create setup ==="

# Git config (safe directory for mounted workspace)
git config --global --add safe.directory "${CONTAINER_WORKSPACE_FOLDER:-/workspaces/live-henkan}"

# Download dictionary data
echo "Setting up dictionary..."
if [ -f scripts/setup-dict.sh ]; then
    chmod +x scripts/setup-dict.sh
    ./scripts/setup-dict.sh
else
    echo "  setup-dict.sh not found, skipping dictionary download."
fi

# Pre-build to warm up the cargo cache in the volume
echo "Warming up cargo cache (initial build)..."
cargo build --workspace 2>/dev/null || echo "  Initial build has todo!() items — this is expected."

echo ""
echo "=== Setup complete ==="
echo "Next steps:"
echo "  1. gh auth login          # Authenticate GitHub CLI"
echo "  2. just check             # Run lint + test"
echo "  3. ./scripts/create-issues.sh  # Create milestone issues"
