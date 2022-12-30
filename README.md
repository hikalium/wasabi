# WasabiOS
Toy web browser and OS in Rust ("sabi" in Japanese) = WasabiOS

## Goals

- Implement a toy OS with just enough features to run a separate toy web browser binary
- Provide sufficient inline comments to explain what the code is doing for beginners
- No external crates (except non-runtime code: e.g. dbgutil)
- Render https://example.com beautifully with a toy browser
- Support some physical x86_64 machine and QEMU
- Keep the code simple as much as possible

## Non-goals

- Replace the Linux kernel (of course it's too difficult, at least for now ;)
- Support all the hardwares in the wild (it's too hard)

## Authors

hikalium, d0iasm


## License

MIT
