#!/bin/bash -e
cd "$(dirname "$(cargo locate-project --message-format plain)")"
cargo --config "target.'cfg(target_os = \"uefi\")'.runner = \"./scripts/install_runner.sh\"" run
