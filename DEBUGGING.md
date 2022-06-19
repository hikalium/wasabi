```
make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1 | cargo run --bin=dbgutil
make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1
```
