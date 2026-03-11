# Justfile for live-henkan development
# Install just: cargo install just

# Run all checks (same as CI)
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# Format and lint
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty

# Run tests for a specific crate
test crate:
    cargo test -p {{crate}}

# Run the TUI prototype
tui:
    cargo run -p tui-prototype

# Setup dictionary data
setup:
    chmod +x scripts/setup-dict.sh
    ./scripts/setup-dict.sh

# Create a feature branch from an issue number
branch issue description:
    git checkout -b feat/{{description}} main
