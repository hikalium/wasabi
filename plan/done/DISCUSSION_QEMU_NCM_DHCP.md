# Discussion log: bundled patched QEMU, USB-NCM networking, and DHCP

This file records a working session (2026-06) that set up a patched QEMU for
running Wasabi under `cargo run`/`cargo test`, fixed the emulated NIC wiring,
diagnosed why `ping` fails under emulation, ported a DHCP client from the
`os/` PoC, and planned an end-to-end DHCP test. It is a narrative record, not
a spec; commits are referenced by subject line because the branch history was
rewritten more than once during the work.

## 1. Bundled patched QEMU (`wasabi_devtools/qemu`)

`code` (this `wasabi` submodule) needs a QEMU built with the `usb-ncm` CDC-NCM
device, which is not in stock QEMU. We keep a self-contained build in a sibling
directory of `code`:

```
manuscript_os/
  code/                      <- this repo (the wasabi submodule)
  wasabi_devtools/qemu/      <- extracted standalone QEMU bundle (gitignored)
    bin/qemu-system-x86_64   <- wrapper -> .real, sets LD_LIBRARY_PATH etc.
    lib/  share/  README.txt
```

`scripts/launch_qemu.sh` prefers it when present, else falls back to PATH:

```sh
QEMU=qemu-system-x86_64
BUNDLED_QEMU="../wasabi_devtools/qemu/bin/qemu-system-x86_64"
if [ -x "${BUNDLED_QEMU}" ]; then
  QEMU="${BUNDLED_QEMU}"
  echo "Using bundled QEMU: ${BUNDLED_QEMU}"
fi
"${QEMU}" \ ...
```

(Commit: *"scripts: prefer bundled patched QEMU when present"*.) The book repo
gitignores `manuscript_os/wasabi_devtools/` — the bundle is a dev tool, intended
to be fetched/generated rather than tracked.

## 2. `package_bundle.sh` fix — missing PC-BIOS ROMs

The first extracted bundle failed at boot with:

```
rom: file kvmvapic.bin : error Failed to open file ...
qemu-system-x86_64.real: failed to find romfile "vgabios-stdvga.bin"
```

Root cause: the bundling script (in the QEMU source tree, `package_bundle.sh`)
dropped **all** firmware blobs (`*.bin *.rom vgabios* seabios* ...`), but QEMU
loads device option ROMs / VGA BIOS / `kvmvapic.bin` at runtime for emulated
devices. Only the large `edk2`/OVMF system firmware is genuinely replaceable
(Wasabi supplies its own OVMF via `-bios`). Fix: narrow the exclusion to
`edk2-*` / `*.fd` and keep the device ROMs. The script's hardcoded paths were
also made overridable via env (`SRC`/`OUT`/`PATCHELF`/`ARCH`) so it can run on
any host.

The bundle was then rebuilt from source on the dev machine to prove the
pipeline: install full dep set → `configure` (gtk/sdl/spice/virgl/vnc/usb-redir/
smartcard/rbd/nfs/iscsi/brlapi/io_uring/seccomp/…) → `ninja` → stage-install →
`package_bundle.sh`. Verified `usb-ncm` present and `ping`-relevant ROMs
included; end-to-end boot via `launch_qemu.sh` reached `Booting WasabiOS...`.

## 3. NIC device wiring: `usb-net` → `usb-ncm`, plus `-netdev user`

- The launch script originally added `-device usb-net`; the patched build
  provides `usb-ncm` (the device Wasabi's driver actually targets). The change
  was folded into the original commit that introduced the device, which now
  reads *"Add usb-ncm device in the QEMU args"*.
- A bare `-device usb-ncm` has no network backend. Added QEMU user-mode
  (SLIRP) networking and attached it:

  ```
  -netdev user,id=usbnet0 \
  -device usb-ncm,netdev=usbnet0 \
  ```

  `-netdev user` gives the guest NAT'd outbound reachability with built-in
  DHCP/DNS, no host setup. Inbound would need `hostfwd=...`.

## 4. Why `ping` fails under emulation (diagnosis only)

Symptom: `ping` from Wasabi receives nothing under QEMU, but works on real
hardware.

Root cause is an **addressing mismatch, not a device bug**:

- Wasabi uses a static `nic::OUR_IP = 10.10.10.83` and treats `10.10.10.1` as
  the peer/gateway (`tcp.rs: PEER_IP`). `ping <ip>` ARPs *from* `10.10.10.83`.
- `-netdev user` (SLIRP) defaults to the `10.0.2.0/24` network. SLIRP only
  answers ARP/ICMP for addresses inside its own subnet and treats a guest
  sourced from off-subnet `10.10.10.83` as unreachable → no ARP reply →
  `ping_once` aborts with `"ARP unresolved"`.

Ruled out a device-level bug: the NTB16 framing in QEMU `ncm-ntb.c`
(`ncm_build_ntb16`) is byte-identical to Wasabi's `src/ncm.rs` parser
(`NCMH`/`NCM0`, block_length@8, ndp_index@10, (idx,len) pairs + (0,0)
terminator), and the device's control/notification/datain paths are complete.

Two fixes were identified (neither committed as part of this diagnosis):

1. **Emulation-side:** match SLIRP's subnet to Wasabi's static config,
   `-netdev user,id=usbnet0,net=10.10.10.0/24,host=10.10.10.1`.
2. **Guest-side (preferred):** make Wasabi do DHCP, so it accepts whatever
   address SLIRP hands out (`10.0.2.15`) and no longer hardcodes `10.10.10.x`.

### Debugging via QEMU trace

The `usb-ncm` device already has trace points: `usb_ncm_tx_frame`
(SLIRP→guest), `usb_ncm_rx_frame` (guest→SLIRP), `usb_ncm_control`,
`usb_ncm_notif_queue/pop`. Enable with `-trace 'usb_ncm_*' -D log/qemu_trace.txt`
and capture the wire with
`-object filter-dump,id=dump0,netdev=usbnet0,file=log/ncm.pcap`. Expected
signature of the bug: after `ping`, `usb_ncm_rx_frame` fires (Wasabi's ARP
leaves) but no `usb_ncm_tx_frame` (SLIRP never replies).

## 5. DHCP client port from the PoC

Ported the DHCP building blocks from the `os/` PoC
(`os/src/net/{dhcp,udp}.rs`) into this tree's **flat** layout (no `net/`
submodule directory):

- `src/udp.rs` — `UdpPacket` (+ `UDP_PORT_DHCP_SERVER/CLIENT`) and
  `UdpSocket`/recv-future.
- `src/dhcp.rs` — `DhcpPacket` + `DhcpPacket::request()`.
- `src/ip.rs` — added `IpV4Protocol::udp()` (=17), which the PoC needed but
  this tree lacked.
- `src/lib.rs` — registered `pub mod dhcp;` and `pub mod udp;`.

Adaptations from the PoC: `crate::net::*` → `crate::*`;
`noli::{mem::Sliceable, net::IpV4Addr}` → `crate::{slice::Sliceable, ip::IpV4Addr}`;
`crate::error::Result` → `crate::result::Result`; `broardcast()` → `broadcast()`;
`from_slice` → `copy_from_slice`; and the IP checksum via
`IpV4Packet::recompute_checksum()` (this tree's equivalent of the PoC's
`clear/set_checksum`). The compile-time `assert!(size_of::<DhcpPacket>() == 282)`
holds. `check.sh` (fmt + clippy `-D warnings` + build + 81 unit tests) passes.

(Commit: *"dhcp: port DHCP request builder and UDP layer from PoC"*.)

**Known gaps in the ported PoC** (carried over verbatim, to be fixed when the
client is wired in):

- `DhcpPacket::request()` builds a bare broadcast `BOOTREQUEST` with the magic
  cookie but **no option fields** — notably no option 53 = `DHCPDISCOVER`.
  SLIRP's DHCP server likely needs a proper DISCOVER to reply.
- There is no OFFER/ACK parsing yet (no read of `yiaddr`/netmask/router/DNS),
  and the client is **not wired into the NIC runtime** — `OUR_IP` is still a
  `const`.

## 6. Plan: end-to-end "Wasabi acquires a DHCP lease in QEMU" test

Goal: a test under `cargo test` that fails unless Wasabi obtains a non-zero
IPv4 lease from SLIRP's DHCP server within a timeout (assert in-subnet
`10.0.2.0/24`, not the static `10.10.10.x`).

### Why it can't be a plain `#[test_case]`

- The `cfg(test)` `efi_main` (lib.rs) only runs `init_basic_runtime` +
  `run_unit_tests` — no `init_acpi/paging/hpet/pci/apic`, so xHCI/USB/NIC never
  come up.
- `#[test_case]`s are sync `Fn()`; `test_runner` never pumps the global
  executor. `block_on` (executor.rs) drives one future and ignores the global
  queue; `Executor::run` is `-> !`.
- But the hardware path *is* driven by the global executor:
  `init_pci` → `PciXhciDriver::attach` → `spawn_global(run)` →
  enumeration → `UsbNcmDriver::start` → `spawn_global(run)`. So **pumping the
  global executor drives the whole xHCI→USB→NIC→DHCP path.**

### Prerequisite (one unit of work with the test)

The DHCP client must be wired into the NIC runtime so there is a real path to
exercise, and must expose an observable result:

- In `nic.rs` bringup, send a DHCP DISCOVER on bulk-OUT and handle inbound
  **UDP dst port 68** in `poll_bulk_in` (today only ARP/ICMP/TCP).
- Fix the PoC gaps from §5 (add option 53 DISCOVER; parse OFFER/ACK `yiaddr`).
- Add `pub static DHCP_LEASE: Mutex<Option<IpV4Addr>>` set on a valid reply.

### Pieces to add

1. `executor::run_global_until(cond, timeout)` — a non-`!` sibling of
   `Executor::run`: pop/poll global tasks, checking `cond()` and an HPET
   deadline; return whether satisfied.
2. Full hardware init in the `cfg(test)` `efi_main` (mirror main.rs's chain,
   minus the app tasks) so the xHCI task is queued before tests run.
3. `#[test_case] fn dhcp_lease_acquired_over_qemu()`:
   `assert!(run_global_until(|| DHCP_LEASE.lock().is_some(), 8s))` then assert
   the lease is non-zero and inside `10.0.2.0/24`. Panic-on-timeout → QEMU Fail.

### Design choice

- **Option A (recommended):** reuse the single test harness — full init in
  `cfg(test)` `efi_main` + the bounded pump + the `#[test_case]`. One EFI,
  integrates with the existing 81 tests and the runner.
- **Option B:** a dedicated integration crate (`tests/dhcp_qemu.rs`) with its
  own `no_main`/test_runner/panic/`efi_main` mirroring main.rs boot. Isolates
  the hardware test but adds no_std/UEFI boilerplate and a second boot.

### Risks / open questions

- SLIRP needs a proper DISCOVER (option 53) — most likely first-attempt
  failure mode; confirm with the trace/pcap from §4.
- Full init inside the test harness may surface ordering/timing quirks; keep
  only what's needed and verify the 81 logic tests still pass.
- Keep the static `OUR_IP` as a fallback so non-network boots/tests don't
  regress.

### Files this would touch

`src/executor.rs`, `src/lib.rs` (`cfg(test)` init), `src/nic.rs` (client +
`DHCP_LEASE`), `src/dhcp.rs` (DISCOVER option + OFFER/ACK parse), and the new
`#[test_case]`.
