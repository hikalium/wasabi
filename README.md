# WasabiOS
Toy web browser and OS in Rust ("sabi" in Japanese) = WasabiOS

## Goals

- Implement a toy OS with just enough features to run a separate toy web browser binary
- Provide sufficient inline comments to explain what the code is doing for beginners
- No external crates (except non-runtime code: e.g. dbgutil)
- Render https://example.com beautifully with a toy browser
- Support some physical x86\_64 machine and QEMU
- Keep the code simple as much as possible

## Non-goals

- Replace the Linux kernel (of course it's too difficult, at least for now ;)
- Support all the hardwares in the wild (it's too hard)

## Getting started

```
make
make run
```
## Running app

```
make internal_run_app_test INIT="app_name_here"
```

## Debugging tips



```
make run MORE_QEMU_FLAGS='-nographic'
make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1 | cargo run --bin=dbgutil
make run MORE_QEMU_FLAGS="-d int,cpu_reset --no-reboot" 2>&1
```

```
tshark -r log/dump_net1.pcap
```

```
(qemu) info usbhost
  Bus 2, Addr 37, Port 3.2.1, Speed 5000 Mb/s
    Class ff: USB device 0b95:1790, AX88179
(qemu) device_add usb-host,hostbus=2,hostport=3.2.1
```

## References
- https://rust-lang.github.io/unsafe-code-guidelines/introduction.html

## Authors

hikalium, d0iasm

## License

MIT
