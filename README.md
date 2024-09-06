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

## Check prerequisites

### Ubuntu

```
rustup --version && \
make --version && \
clang --version && \
qemu-system-x86_64 --version && \
echo "This machine is ready to build WasabiOS!"
```

- `rustup` can be installed via: https://www.rust-lang.org/tools/install
- `make` can be installed with: `sudo apt install make`
- `clang` can be installed with: `sudo apt install clang`
- `qemu-system-x86_64` can be installed with: `sudo apt install qemu-system-x86`

### Crostini (Linux environment on ChromeOS)

```
./scripts/setup_dev_crostini.sh
bash # to reload environment variables
```

### [beta] macOS

```
./scripts/setup_dev_macos.sh
zsh # to reload environment variables
```

## Build and run WasabiOS on QEMU

```
make run
```

### Running app

```
make internal_run_app_test INIT="app_name_here"
```

## Build an app for WasabiOS

For example, to create a "hello2" app, run following commands outside of WasabiOS checkout:

```
cargo new hello2
cd hello2
cargo add noli --git https://github.com/hikalium/wasabi.git
rustup target add x86_64-unknown-none
wget https://raw.githubusercontent.com/hikalium/wasabi/main/rust-toolchain.toml
wget https://raw.githubusercontent.com/hikalium/wasabi/main/external_app_template/Makefile
```

Then, please make sure following lines are added at the beginning of src/main.rs:
```rust
#![no_std]
#![cfg_attr(not(target_os = "linux"), no_main)]
use noli::prelude::*;
entry_point!(main);
```

Now, you can run apps with:

```
make
/path/to/wasabi/scripts/run_with_app.sh ./target/x86_64-unknown-none/release/hello2
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

## Run pre-commit checks
```
make commit
```

## Run CI job locally
After installing [GitHub CLI](https://github.com/cli/cli?tab=readme-ov-file#installation) and [Docker](https://docs.docker.com/engine/install/), run:
```
# Check if Docker works without root
docker run hello-world

# Install act extention
gh extension install https://github.com/nektos/gh-act

# Run
gh act

# Run without docker pull (useful when uprev the container)
gh act --pull=false
```

## FAQ

### Neovim + coc-rust-analyzer fails
Try uninstalling all the toolchains installed and `make` again to install the desired version, with this:
```
rustup toolchain uninstall `rustup toolchain list | cut -d ' ' -f 1`
make
```

Also, it is possible that your neovim installation is outdated.

FYI: hikalium is using [this version of neovim](https://github.com/hikalium/dotfiles/blob/master/scripts/setup_nvim_linux.sh#L2).

## References
- https://rust-lang.github.io/unsafe-code-guidelines/introduction.html
- https://www.cqpub.co.jp/interface/sample/200512/if0512_chap1.pdf
- https://en.wikipedia.org/wiki/File:Tcp_state_diagram.png

## Authors

hikalium, d0iasm

## License

MIT
