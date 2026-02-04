#!/bin/bash
# Test script for pyro-ld

set -e

echo "Building pyro-ld..."
cargo build --release -p pyro-ld

LINKER="./target/release/ld-pyro"

if [ ! -f "$LINKER" ]; then
    echo "Error: Linker not found at $LINKER"
    exit 1
fi

echo "Linker built successfully: $LINKER"

echo ""
echo "To test the linker:"
echo "1. Build a simple program:"
echo "   gcc -o test tests/test_minimal.c"
echo ""
echo "2. Use pyro-ld as interpreter:"
echo "   gcc -o test tests/test_minimal.c -Wl,--dynamic-linker,$LINKER"
echo "   ./test"
echo ""
echo "Note: Current implementation can only bootstrap itself."
echo "Full program loading requires additional implementation."
