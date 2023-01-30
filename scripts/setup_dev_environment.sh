#!/bin/bash -e
cat /etc/lsb-release | grep 'Ubuntu 20.04' || { echo "This script currently supports Ubuntu 20.04 only. Please read ./scripts/setup_dev_environments.sh manually to find out what steps are needed." ; exit 1 ; }
echo "Installing dev tools..."

source "$HOME/.cargo/env"
which rustup && echo "rustup is installed already" || {
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
}

rustup update

echo "Please make sure to restart your shell before building something"