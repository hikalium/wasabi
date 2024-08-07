#!/bin/bash -e
{ test -f /dev/.cros_milestone ; } || { echo "This script supports Linux environment on ChromeOS (crostini) only. Please read the script manually to find out what steps are needed." ; exit 1 ; }

echo "Installing dev tools..."
sudo apt install -y build-essential qemu-system-x86 netcat-openbsd

which rustup && echo "rustup is installed already" || {
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
}

test -f "$HOME/.cargo/env" && source "$HOME/.cargo/env" || true
rustup update

echo "Please make sure to restart your shell before building something"
