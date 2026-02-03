#!/bin/bash
set -e

# Install curl if not available
if command -v apk &> /dev/null; then
    apk add --no-cache curl bash
elif command -v apt-get &> /dev/null; then
    apt-get update
    apt-get install -y curl
fi

# Install Claude Code using the official installer

curl -fsSL https://claude.ai/install.sh | bash


echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc