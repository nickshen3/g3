#!/bin/bash
# Build and install g3 and studio to ~/.local/bin

set -e

cd "$(dirname "$0")/.."

INSTALL_DIR="$HOME/.local/bin"
mkdir -p "$INSTALL_DIR"

echo "Building g3 and studio (release)..."
cargo build --release -p g3 -p studio

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

# Check if ~/.local/bin is in PATH and fix if needed
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "⚠️  $INSTALL_DIR is not in your PATH"
    
    # Detect shell config file
    SHELL_NAME=$(basename "$SHELL")
    case "$SHELL_NAME" in
        zsh)  RC_FILE="$HOME/.zshrc" ;;
        bash) 
            # macOS uses .bash_profile, Linux uses .bashrc
            if [[ "$OSTYPE" == "darwin"* ]]; then
                RC_FILE="$HOME/.bash_profile"
            else
                RC_FILE="$HOME/.bashrc"
            fi
            ;;
        fish) RC_FILE="$HOME/.config/fish/config.fish" ;;
        *)    RC_FILE="" ;;
    esac
    
    if [ -n "$RC_FILE" ]; then
        # Check if it's already in the rc file (just not loaded in current session)
        if grep -q '\.local/bin' "$RC_FILE" 2>/dev/null; then
            echo "   (Already in $RC_FILE, just not loaded in this session)"
            echo "   Run: source $RC_FILE"
        else
            echo ""
            read -p "   Add to $RC_FILE? [Y/n] " -n 1 -r
            echo
            if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
                echo '' >> "$RC_FILE"
                if [[ "$SHELL_NAME" == "fish" ]]; then
                    echo 'set -gx PATH $HOME/.local/bin $PATH' >> "$RC_FILE"
                else
                    echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$RC_FILE"
                fi
                echo "   ✅ Added to $RC_FILE"
                echo "   Run: source $RC_FILE"
            else
                echo "   Skipped. Add manually:"
                echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
            fi
        fi
    else
        echo "   Unknown shell ($SHELL_NAME). Add this to your shell rc file:"
        echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
fi
