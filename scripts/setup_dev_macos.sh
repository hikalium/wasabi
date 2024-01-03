#!/bin/bash -e
{ sw_vers | grep macOS ; } || { echo "This script supports macOS environment only. Please read the script manually to find out what steps are needed." ; exit 1 ; }

{ /opt/homebrew/bin/brew --version | grep Homebrew ; } || { echo "Please install Homebrew package manager by following instructions on https://brew.sh/" ; exit 1 ; }

{ brew --version | grep Homebrew ; } || { echo 'Please run `eval "$(/opt/homebrew/bin/brew shellenv)"` to setup your environment variable correctly.' ; exit 1 ; }

echo "Installing dev tools..."
brew install qemu make

which rustup && echo "rustup is installed already" || {
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
}

test -f "$HOME/.cargo/env" && source "$HOME/.cargo/env" || true
rustup update

echo "Please make sure to restart your shell before building something"
