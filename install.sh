#!/bin/sh
set -e

main() {
    echo "Installing dek..."

    # Check for cargo
    if command -v cargo >/dev/null 2>&1; then
        # Try binstall first (faster, pre-compiled)
        if command -v cargo-binstall >/dev/null 2>&1; then
            echo "Using cargo binstall..."
            cargo binstall -y dek
        else
            echo "Using cargo install..."
            cargo install dek
        fi
    else
        echo "Cargo not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        . "$HOME/.cargo/env"
        cargo install dek
    fi

    echo ""
    echo "dek installed successfully!"
    echo "Run 'dek --help' to get started"
}

main
