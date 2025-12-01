#!/usr/bin/env bash
#
# Filen Menubar - Linux Install Script
# Builds from source and sets up autostart
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

INSTALL_DIR="/usr/local/bin"
DESKTOP_FILE="$HOME/.local/share/applications/filen-menubar.desktop"
AUTOSTART_FILE="$HOME/.config/autostart/filen-menubar.desktop"
ICON_DIR="$HOME/.local/share/icons/hicolor"

print_step() {
    echo -e "${BLUE}==>${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}!${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

# Detect package manager and distro
detect_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        DISTRO=$ID
        DISTRO_LIKE=$ID_LIKE
    else
        DISTRO="unknown"
        DISTRO_LIKE=""
    fi
}

# Install dependencies based on distro
install_dependencies() {
    print_step "Checking and installing dependencies..."

    detect_distro

    case "$DISTRO" in
        arch|cachyos|manjaro|endeavouros|garuda)
            print_step "Detected Arch-based distro: $DISTRO"
            if ! pacman -Q webkit2gtk-4.1 &>/dev/null; then
                print_warning "Installing build dependencies (requires sudo)..."
                sudo pacman -S --needed webkit2gtk-4.1 libappindicator-gtk3 librsvg \
                    base-devel curl wget file openssl libxdo nodejs npm rust
            else
                print_success "Dependencies already installed"
            fi
            ;;
        fedora|rhel|centos)
            print_step "Detected Fedora/RHEL-based distro: $DISTRO"
            print_warning "Installing build dependencies (requires sudo)..."
            sudo dnf install -y webkit2gtk4.1-devel openssl-devel curl wget file \
                libxdo-devel libappindicator-gtk3-devel librsvg2-devel nodejs npm rust cargo
            ;;
        ubuntu|debian|pop|linuxmint|elementary)
            print_step "Detected Debian-based distro: $DISTRO"
            print_warning "Installing build dependencies (requires sudo)..."
            sudo apt update
            sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
                libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev nodejs npm
            # Rust via rustup on Debian-based
            if ! command -v cargo &>/dev/null; then
                print_step "Installing Rust via rustup..."
                curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
                source "$HOME/.cargo/env"
            fi
            ;;
        *)
            # Check ID_LIKE for derivative distros
            if [[ "$DISTRO_LIKE" == *"arch"* ]]; then
                print_step "Detected Arch-based distro (via ID_LIKE): $DISTRO"
                sudo pacman -S --needed webkit2gtk-4.1 libappindicator-gtk3 librsvg \
                    base-devel curl wget file openssl libxdo nodejs npm rust
            elif [[ "$DISTRO_LIKE" == *"debian"* ]] || [[ "$DISTRO_LIKE" == *"ubuntu"* ]]; then
                print_step "Detected Debian-based distro (via ID_LIKE): $DISTRO"
                sudo apt update
                sudo apt install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
                    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev nodejs npm
            else
                print_error "Unknown distro: $DISTRO"
                print_warning "Please install dependencies manually. See README.md"
                exit 1
            fi
            ;;
    esac
}

# Check for Filen CLI
check_filen_cli() {
    print_step "Checking for Filen CLI..."

    if command -v filen &>/dev/null; then
        FILEN_VERSION=$(filen --version 2>/dev/null || echo "unknown")
        print_success "Filen CLI found: $FILEN_VERSION"
    else
        print_warning "Filen CLI not found. Installing..."
        npm install -g @filen/cli
        print_success "Filen CLI installed"
        echo ""
        print_warning "IMPORTANT: You need to login to Filen CLI before using the menubar app:"
        echo "    filen"
        echo "    (This opens an interactive session where you can login)"
        echo ""
    fi
}

# Build the application
build_app() {
    print_step "Building Filen Menubar..."

    # Get script directory and project root
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

    cd "$PROJECT_ROOT"

    # Install npm dependencies
    print_step "Installing npm dependencies..."
    npm install

    # Install Tauri CLI if not present
    if ! command -v cargo-tauri &>/dev/null; then
        print_step "Installing Tauri CLI..."
        cargo install tauri-cli --version "^2" --locked
    fi

    # Build release
    print_step "Building release binary (this may take a few minutes)..."
    npm run tauri build

    BINARY_PATH="$PROJECT_ROOT/src-tauri/target/release/filen-menubar"

    if [ -f "$BINARY_PATH" ]; then
        print_success "Build successful!"
    else
        print_error "Build failed - binary not found"
        exit 1
    fi
}

# Install the binary
install_binary() {
    print_step "Installing binary to $INSTALL_DIR..."

    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
    BINARY_PATH="$PROJECT_ROOT/src-tauri/target/release/filen-menubar"

    sudo install -Dm755 "$BINARY_PATH" "$INSTALL_DIR/filen-menubar"
    print_success "Binary installed to $INSTALL_DIR/filen-menubar"
}

# Install icons
install_icons() {
    print_step "Installing icons..."

    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
    ICONS_DIR="$PROJECT_ROOT/src-tauri/icons"

    mkdir -p "$ICON_DIR/32x32/apps"
    mkdir -p "$ICON_DIR/128x128/apps"
    mkdir -p "$ICON_DIR/256x256/apps"

    if [ -f "$ICONS_DIR/32x32.png" ]; then
        cp "$ICONS_DIR/32x32.png" "$ICON_DIR/32x32/apps/filen-menubar.png"
    fi
    if [ -f "$ICONS_DIR/128x128.png" ]; then
        cp "$ICONS_DIR/128x128.png" "$ICON_DIR/128x128/apps/filen-menubar.png"
    fi
    if [ -f "$ICONS_DIR/128x128@2x.png" ]; then
        cp "$ICONS_DIR/128x128@2x.png" "$ICON_DIR/256x256/apps/filen-menubar.png"
    fi

    # Update icon cache if available
    if command -v gtk-update-icon-cache &>/dev/null; then
        gtk-update-icon-cache -f -t "$ICON_DIR" 2>/dev/null || true
    fi

    print_success "Icons installed"
}

# Create desktop entry
create_desktop_entry() {
    print_step "Creating desktop entry..."

    mkdir -p "$(dirname "$DESKTOP_FILE")"

    cat > "$DESKTOP_FILE" << EOF
[Desktop Entry]
Name=Filen Menubar
Comment=Lightweight menubar app for Filen cloud sync
Exec=filen-menubar
Icon=filen-menubar
Type=Application
Terminal=false
Categories=Network;FileTransfer;Utility;
Keywords=filen;sync;cloud;backup;
StartupNotify=false
StartupWMClass=filen-menubar
EOF

    print_success "Desktop entry created at $DESKTOP_FILE"
}

# Setup autostart
setup_autostart() {
    print_step "Setting up autostart..."

    mkdir -p "$(dirname "$AUTOSTART_FILE")"

    cat > "$AUTOSTART_FILE" << EOF
[Desktop Entry]
Name=Filen Menubar
Comment=Lightweight menubar app for Filen cloud sync
Exec=filen-menubar
Icon=filen-menubar
Type=Application
Terminal=false
X-GNOME-Autostart-enabled=true
StartupNotify=false
EOF

    print_success "Autostart configured at $AUTOSTART_FILE"
}

# Uninstall function
uninstall() {
    print_step "Uninstalling Filen Menubar..."

    # Kill running instance
    pkill -f filen-menubar 2>/dev/null || true

    # Remove binary
    if [ -f "$INSTALL_DIR/filen-menubar" ]; then
        sudo rm -f "$INSTALL_DIR/filen-menubar"
        print_success "Removed binary"
    fi

    # Remove desktop entry
    if [ -f "$DESKTOP_FILE" ]; then
        rm -f "$DESKTOP_FILE"
        print_success "Removed desktop entry"
    fi

    # Remove autostart
    if [ -f "$AUTOSTART_FILE" ]; then
        rm -f "$AUTOSTART_FILE"
        print_success "Removed autostart entry"
    fi

    # Remove icons
    rm -f "$ICON_DIR/32x32/apps/filen-menubar.png" 2>/dev/null || true
    rm -f "$ICON_DIR/128x128/apps/filen-menubar.png" 2>/dev/null || true
    rm -f "$ICON_DIR/256x256/apps/filen-menubar.png" 2>/dev/null || true
    print_success "Removed icons"

    print_success "Uninstall complete!"
}

# Show usage
usage() {
    echo "Filen Menubar - Linux Install Script"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  install     Full install (dependencies, build, install, autostart)"
    echo "  build       Build only (no install)"
    echo "  setup       Install pre-built binary and setup autostart"
    echo "  autostart   Setup autostart only"
    echo "  uninstall   Remove Filen Menubar"
    echo "  deps        Install dependencies only"
    echo ""
    echo "Examples:"
    echo "  $0 install     # Full installation from source"
    echo "  $0 setup       # Setup after manual build"
    echo "  $0 uninstall   # Remove everything"
}

# Main
main() {
    echo ""
    echo "╔═══════════════════════════════════════╗"
    echo "║       Filen Menubar Installer         ║"
    echo "╚═══════════════════════════════════════╝"
    echo ""

    case "${1:-install}" in
        install)
            install_dependencies
            check_filen_cli
            build_app
            install_binary
            install_icons
            create_desktop_entry
            setup_autostart
            echo ""
            print_success "Installation complete!"
            echo ""
            echo "You can now:"
            echo "  • Run 'filen-menubar' from terminal"
            echo "  • Find 'Filen Menubar' in your application menu"
            echo "  • It will start automatically on login"
            echo ""
            if ! filen whoami &>/dev/null 2>&1; then
                print_warning "Don't forget to login by running: filen"
            fi
            ;;
        build)
            install_dependencies
            build_app
            echo ""
            print_success "Build complete!"
            echo "Binary at: src-tauri/target/release/filen-menubar"
            ;;
        setup)
            install_binary
            install_icons
            create_desktop_entry
            setup_autostart
            echo ""
            print_success "Setup complete!"
            ;;
        autostart)
            setup_autostart
            print_success "Autostart enabled!"
            ;;
        uninstall)
            uninstall
            ;;
        deps)
            install_dependencies
            check_filen_cli
            print_success "Dependencies installed!"
            ;;
        -h|--help|help)
            usage
            ;;
        *)
            print_error "Unknown command: $1"
            usage
            exit 1
            ;;
    esac
}

main "$@"
