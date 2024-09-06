#!/bin/bash -e
WASABI_DIR="$(dirname $(dirname ${BASH_SOURCE:-$0}))"
echo "Using wasabi at: ${WASABI_DIR}"
WITH_APP_BIN="$(readlink -f $1)"
[ -f "${WITH_APP_BIN}" ] || echo "Please specify a path to app bin" || echo "Using app at: ${WITH_APP_BIN}"

make -C "${WASABI_DIR}" run WITH_APP_BIN="${WITH_APP_BIN}"
