#!/bin/bash
# Open log folder for the current platform
# Usage: ./scripts/logs.sh

set -e

# Determine log directory based on platform
case "$(uname -s)" in
    Darwin)
        LOG_DIR="$HOME/Library/Logs/io.filen.menubar"
        ;;
    Linux)
        LOG_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/filen-menubar/logs"
        ;;
    *)
        echo "Unsupported platform: $(uname -s)"
        exit 1
        ;;
esac

# Create the directory if it doesn't exist
mkdir -p "$LOG_DIR"

echo "Log directory: $LOG_DIR"

# Open the folder
case "$(uname -s)" in
    Darwin)
        open "$LOG_DIR"
        ;;
    Linux)
        xdg-open "$LOG_DIR" 2>/dev/null || \
        nautilus "$LOG_DIR" 2>/dev/null || \
        dolphin "$LOG_DIR" 2>/dev/null || \
        thunar "$LOG_DIR" 2>/dev/null || \
        echo "Could not open file manager. Log directory: $LOG_DIR"
        ;;
esac
