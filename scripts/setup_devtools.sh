#!/bin/bash -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST_DIR="$SCRIPT_DIR/../../wasabi_devtools/qemu"
RELEASE_URL="https://github.com/hikalium/qemu/releases/download/v11.0.50-wasabi/wasabi-qemu-x86_64-linux.tar.gz"
TEMP_TARBALL="/tmp/wasabi-qemu-x86_64-linux.tar.gz"

echo "=== Downloading QEMU bundle from GitHub Release ==="
echo "URL: $RELEASE_URL"
curl -L -o "$TEMP_TARBALL" "$RELEASE_URL"

echo "=== Unpacking QEMU bundle ==="
echo "Destination: $DEST_DIR"
mkdir -p "$DEST_DIR"
# Clean previous unpack if exists
rm -rf "${DEST_DIR:?}"/*
tar xzf "$TEMP_TARBALL" --strip-components=1 -C "$DEST_DIR"

# Clean up temp file
rm -f "$TEMP_TARBALL"

echo "=== Verification ==="
if "$DEST_DIR/bin/qemu-system-x86_64" --version > /dev/null; then
    echo "Success! QEMU unpacked and verified."
    "$DEST_DIR/bin/qemu-system-x86_64" --version | head -n 1
else
    echo "Error: QEMU verification failed."
    exit 1
fi
