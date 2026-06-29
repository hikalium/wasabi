#!/bin/bash -e
PROJ_ROOT="$(dirname $(dirname ${BASH_SOURCE:-$0}))"
cd "${PROJ_ROOT}"

if ! [ -f ./Cargo.toml ] ; then
    echo "## ./Cargo.toml not populated yet. Skip checking."
    exit 0
fi

echo "Checking: `git show --oneline -s HEAD`"

cargo fmt --check
cargo clippy -- -D warnings -A clippy::empty-loop

if [ "${SKIP_TEST}" = "1" ] ; then
    echo "## SKIP_TEST env var is set to 1. Skip running tests."
    exit 0
fi

if git show --oneline -s HEAD | grep 'SKIP_TEST:' ; then
    echo "## SKIP_TEST: marker found. Skip running tests."
    exit 0
fi

# Delete mnt (especially mnt/NvVars) to avoid some stateful issues
# (e.g. boot into EFI shell instead of OS)
rm -rf mnt

cargo build
rm -rf mnt && cargo test
echo "## This looks OK ;)"
