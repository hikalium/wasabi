# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

WasabiOS is a bare-metal OS kernel written in Rust targeting x86-64, designed as educational material for the Wasabi Book. It boots via UEFI firmware and runs on real hardware or QEMU.

## Build & Run Commands

```bash
cargo build                  # Build the EFI binary
cargo test                   # Build and run tests in QEMU
cargo fmt --check            # Check formatting
cargo clippy -- -D warnings -A clippy::empty-loop  # Lint
./scripts/check.sh           # Full CI check (fmt + clippy + build + test)
./scripts/install.sh         # Install to USB disk (WASABIOS partition)
```

**Toolchain**: Nightly Rust pinned to `nightly-2024-01-01` (see `rust-toolchain.toml`).
**Target**: `x86_64-unknown-uefi` (configured in `.cargo/config.toml`).

QEMU is invoked automatically by `cargo test` via `scripts/launch_qemu.sh`. Exit code 3 from QEMU means PASS. Delete `mnt/` before test runs to avoid stateful issues (e.g., `rm -rf mnt && cargo test`).

## Commit Message Conventions

- `SKIP_TEST:` prefix — skips running tests in CI
- `SKIP_EXPLAIN:` prefix — omits commit from book explanation

## Splitting Commits for the Book

Commits are intentionally kept small and self-contained so each one can be explained as a chapter step. When splitting a large WIP commit:

1. **Create a dated temp branch** from the parent of the target commit:
   ```bash
   git checkout -b split-usb-YYYYMMDD <parent-sha>
   ```
2. **Apply changes file-by-file / hunk-by-hunk**, committing each logical slice.
3. **Cherry-pick subsequent commits** on top once the split is done:
   ```bash
   git cherry-pick <wip-sha>..wasabi_book_dev
   ```
4. **Validate every intermediate commit** — each must compile, pass fmt, and pass clippy:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings -A clippy::empty-loop
   rm -rf mnt && cargo test
   git diff wasabi_book_dev HEAD  # should be empty (or only intentional changes)
   ```
5. When splitting a commit that touches files with **cross-file API changes** (e.g. adding a parameter to a trait), all call sites must be updated in the same commit. It is fine to apply the minimal required change to files whose full refactor is deferred to a later split commit.

**Cross-file dependency rule**: if changing xhci.rs updates a method signature that usb.rs or keyboard.rs calls, those callers must be updated in the same commit — even if only a stub/minimal update — so the intermediate state compiles.

## Architecture

### Boot & Initialization (`src/main.rs`, `src/init.rs`)

`efi_main()` is the UEFI entry point. Initialization sequence:
1. Display/VRAM setup via UEFI Graphics Output Protocol
2. Exit UEFI boot services (`init_basic_runtime`)
3. Page tables and virtual memory (`init_paging`)
4. Heap allocator from EFI memory map (`init_allocator`)
5. ACPI table parsing → PCI enumeration → APIC setup
6. Async tasks spawned for USB keyboard, PS/2 keyboard, USB NIC, serial monitoring
7. Global async executor starts

### Async Runtime (`src/executor.rs`)

Custom `#![no_std]` async executor — no Tokio or async-std. `Task<T>` wraps futures with source location for debugging. `spawn_global()` / `start_global_executor()` drive the task queue. `sleep()` uses HPET timer.

### Hardware Drivers

| Module | Role |
|---|---|
| `src/x86.rs` | CPU instructions, page tables (PML4), Local APIC timer |
| `src/xhci.rs` | USB 3.0 xHCI host controller driver (largest file, ~84KB) |
| `src/usb.rs` | USB descriptor parsing, device enumeration, HID |
| `src/pci.rs` | PCI bus enumeration via ACPI MCFG table |
| `src/acpi.rs` | ACPI table parsing (RSDP, FADT, MCFG, HPET) |
| `src/hpet.rs` | High Precision Event Timer — used for `sleep()` |
| `src/uefi.rs` | UEFI protocol bindings (system table, GOP, memory map) |

### Memory & Safety Primitives

- `src/allocator.rs` — Custom heap allocator, registered as global `#[global_allocator]`
- `src/mmio.rs` — `Mmio<T>` / `IoBox<T>` for safe MMIO access
- `src/volatile.rs` — Volatile read/write wrappers for hardware registers
- `src/mutex.rs` — Bare-metal `Mutex<T>`

### Graphics & UI (`src/graphics.rs`, `src/cui.rs`, `src/font.rs`)

`Bitmap` trait abstracts pixel operations. `cui.rs` provides a terminal-style text layer with scrolling and cursor. Font rendering uses `third_party/unifont/` glyphs (8x16 and 16x16); glyph data lives in `src/glyphs.txt`.

### Test Framework (`src/test_runner.rs`, `src/qemu.rs`)

Custom `#![test_runner]` — tests run in QEMU and exit via `isa-debug-exit` device. Pass = QEMU exit 3.

## Key Structural Notes

- **`src/lib.rs`** declares all modules; **`src/main.rs`** and test configurations use the lib crate.
- All hardware access goes through volatile/MMIO wrappers — avoid raw pointer casts without them.
- The kernel is `#![no_std]` + `#![no_main]`; no OS primitives are available — anything OS-like is implemented here.
- QEMU monitor available at `telnet localhost 2345` during a run; serial log saved to `log/com1.txt`.
