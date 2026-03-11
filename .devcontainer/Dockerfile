FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# System dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Rust components (fmt + clippy are already in the base image, but ensure latest)
RUN rustup component add rustfmt clippy

# Windows cross-compilation target (for future TSF frontend development)
# Uncomment when M7 work begins:
# RUN rustup target add x86_64-pc-windows-msvc

# Developer tools
RUN cargo install just cargo-watch

# Ensure cargo bin is in PATH for all users
ENV PATH="/usr/local/cargo/bin:${PATH}"
