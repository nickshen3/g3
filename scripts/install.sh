#!/bin/bash
# Build and install g3 and studio to ~/.local/bin

set -e

cd "$(dirname "$0")/.."

INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"

echo "Building g3 and studio (release)..."
cargo build --release

echo "Installing to $INSTALL_DIR..."
cp target/release/g3 "$INSTALL_DIR/"
cp target/release/studio "$INSTALL_DIR/g3-studio"

# Re-sign binaries after copying (required on macOS to avoid security policy rejection)
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "Re-signing binaries for macOS..."
    codesign --force --sign - "$INSTALL_DIR/g3"
    codesign --force --sign - "$INSTALL_DIR/g3-studio"
fi

# Create symlink to override Android Studio's 'studio' command
# Remove existing symlink if present, but don't remove if it's a different file
if [ -L "$INSTALL_DIR/studio" ]; then
    rm "$INSTALL_DIR/studio"
fi
ln -s "$INSTALL_DIR/g3-studio" "$INSTALL_DIR/studio"

echo "Done! Installed:"
echo "  $INSTALL_DIR/g3"
echo "  $INSTALL_DIR/g3-studio"
echo "  $INSTALL_DIR/studio -> g3-studio"

# Check if ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "⚠️  $INSTALL_DIR is not in your PATH"
    echo "   Add this to your shell rc file:"
    echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
fi
