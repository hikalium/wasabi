# PLAN: `ping_to_gateway` integration test (rev. 3)

Status: **DRAFT — awaiting review. Do not implement yet.**

## Goal

Add a lightweight, in-OS integration test runnable as:

```
cargo test --test ping_to_gateway
```

It boots WasabiOS under QEMU (the normal full boot, so USB→NCM→DHCP comes
up), schedules a test task *between* system setup and the executor main
loop, pings the DHCP-learned **default gateway**, and quits QEMU with
success/failure. Much lighter than the old host-side `e2etest/` crate
(std+tokio, drives QEMU over serial/monitor); here the verdict is reported
from inside the OS via `isa-debug-exit`, which `scripts/launch_qemu.sh`
already turns into a shell exit code.

This code is **book content** (explained in the manuscript like the rest
of the OS), not throwaway — so it is committed at a fine, individually
explainable granularity.

## Established facts (from the current tree)

- `scripts/launch_qemu.sh` is the cargo runner for `cfg(target_os="uefi")`;
  it attaches slirp networking (`usb-ncm`, gateway `10.0.2.2` via DHCP
  `DHCP_OPT_ROUTER`) and `isa-debug-exit`, and maps QEMU status **3→PASS
  (shell 0)** / **5→FAIL (shell 1)**.
- `wasabi::qemu::exit_qemu(Success=0x1→status3 / Fail=0x2→status5)` lets the
  OS decide the verdict.
- Unit tests (`#[test_case]`) run under a **minimal** `cfg(test)` `efi_main`
  (`run_unit_tests()` only) — no NIC/DHCP/executor — so a real gateway ping
  cannot be a unit test; it needs the full boot.
- `src/main.rs::efi_main` is the full boot: `init_*` sequence,
  `spawn_global(...)` of the default tasks, then `start_global_executor()`.
  The NCM NIC self-starts during USB enumeration (`nic.rs Self::run()`), so
  a main-equivalent boot brings networking up by itself.
- Ping primitives already exist in `src/cui.rs::ping_once`
  (`icmp::echo_request_frame` + `nic::{our_mac,our_ip,next_hop,arp_lookup,
  enqueue_tx_frame,PING_PENDING}`); gateway accessor is `nic::router()`.

## Settled design (per review)

### 1. Split the boot into two library functions (reuse, no `extra` param)

Introduce in a new `src/boot.rs` (`wasabi::boot`):

- `pub fn setup_system(image_handle: EfiHandle, est: &EfiSystemTable)`
  — performs the whole `init_*` sequence and `spawn_global(...)`s the
  default tasks (serial monitor, input, ps2kbd, abp_uart). No `extra`
  argument; returns `()`. (Moves the body of today's `main.rs::efi_main`.)
- `pub fn run_system() -> !`
  — runs the OS main executor loop (`start_global_executor()`).

`src/main.rs::efi_main` becomes simply:

```rust
fn efi_main(image_handle: EfiHandle, est: &EfiSystemTable) {
    wasabi::boot::setup_system(image_handle, est);
    wasabi::boot::run_system();
}
```

The integration test slips its own work **between** the two calls — the
global executor isn't running yet, so `spawn_global` just queues the task
for `run_system()` to pick up:

```rust
fn efi_main(image_handle: EfiHandle, est: &EfiSystemTable) {
    wasabi::boot::setup_system(image_handle, est);
    spawn_global(ping_gateway_test());     // scheduled before the loop
    spawn_global(watchdog(WATCHDOG));      // safety net (see §4)
    wasabi::boot::run_system();
}
```

### 2. Extract a result-returning ping core into a new `net.rs` (option 2a)

Move the ICMP send/await orchestration out of `src/cui.rs` into a **new
`src/net.rs` (`wasabi::net`)** as a reusable function:

```rust
pub async fn ping_once_result(target: IpV4Addr, seq: u16)
    -> Result<Option<Duration>>;   // Ok(Some(rtt)) = reply, Ok(None) = timeout
```

`src/cui.rs::run_cmd_ping`/`ping_task` keep only the user-facing printing
and call `net::ping_once_result`; the integration test calls the same
function (no logic duplication).

**Cohesion rationale (why `net.rs`, not `icmp.rs`):** `icmp.rs` is today a
pure packet-format module — it depends only on `checksum`/`eth`/`ip` and
knows nothing about the NIC or ARP. `ping_once_result` is cross-layer: it
needs `icmp::echo_request_frame` **and** `nic::{our_mac,our_ip,next_hop,
arp_lookup,enqueue_tx_frame,PING_PENDING}` **and** `arp::ArpPacket` **and**
`hpet`/`executor`. Putting it in `icmp.rs` would drag NIC/ARP/runtime
dependencies into the packet module and erode its single responsibility.
A `net.rs` module for higher-level network *operations* is the cohesive
owner of such orchestration (ping today, room for more later), leaving
`icmp.rs` purely about ICMP packets.

### 3. The test task

```rust
async fn ping_gateway_test() -> ! {
    // a) wait for DHCP: poll until nic::has_ip() && nic::router().is_some(),
    //    bounded by DHCP_WAIT (~10 s). On timeout: log + exit_qemu(Fail).
    // b) let gw = nic::router().unwrap();   // default gateway (on-subnet)
    // c) for seq in 1..=N { if ping_once_result(gw, seq).await?.is_some()
    //                          { println!("PASS ..."); exit_qemu(Success) } }
    // d) no reply across N tries -> println!("FAIL ..."); exit_qemu(Fail)
}
```

### 4. Termination / safety (in-OS watchdog; no harness timeout available)

Investigated: there is **no timeout mechanism** today — `.cargo/config.toml`
has none (cargo has no runner-timeout setting) and `scripts/launch_qemu.sh`
does not wrap QEMU in `timeout`. A blanket `timeout` in `launch_qemu.sh`
would be wrong because the same runner serves interactive `cargo run`
(it would kill the interactive OS). So termination is handled **inside the
test**:

- `ping_once_result` is bounded (1 s reply timeout) and the DHCP wait is
  bounded (~10 s), so the task always reaches a verdict → `exit_qemu`.
- A small `watchdog(timeout)` task (sleep → `exit_qemu(Fail)`) guards
  against an unexpected stall before any verdict is produced. This lives in
  the test binary only, so it never affects interactive runs.
- The test binary defines its own `#[panic_handler]` → `exit_qemu(Fail)`
  (the library carries no panic handler outside `cfg(test)`, so the
  integration-test binary supplies one, exactly like `main.rs`).

If a harness-level guard is ever wanted, wrap the *invocation*
(`timeout 60 cargo test --test ping_to_gateway`) rather than the shared
runner.

### 5. Cargo wiring

```toml
[[test]]
name = "ping_to_gateway"
harness = false        # our own #[no_mangle] efi_main, no libtest
```

`tests/ping_to_gateway.rs`: `#![no_std] #![no_main]`, the `efi_main` shown
in §1, the `ping_gateway_test`/`watchdog` tasks, and a `#[panic_handler]`.
Built for the uefi target and launched through the existing runner;
`cargo test --test ping_to_gateway` → boot → verdict → exit code.

### 6. No special handling for the per-commit gate or all-history check

Deliberately **no** change to `scripts/check.sh` and **no** exclusion from
`scripts/check_all_commits.sh`. Rationale (per review): an integration test
is introduced *after* the feature it exercises, so on any commit where the
test file exists, the ping/DHCP path also exists and the test passes; on
earlier commits the test file simply isn't there and `cargo test` doesn't
run it. Commit ordering already enforces this, so **`cargo test` stays
green on every commit** with no extra machinery.

## Commit plan (fine-grained, each explainable & check.sh-green)

All three commits are labeled **`book2/ch5:`** and land **at the end of
ch5, as the natural "extend the tests" continuation** of the network
chapter (per review). They sit at HEAD, after the ping/DHCP-gateway
commits, and **before** the high-risk CLI-command reorder (so the test is a
regression net for that reorder).

1. **`book2/ch5: boot: extract setup_system()/run_system(); use them in
   main.rs`** — pure refactor, no behavior change. Explained in ch5 as the
   restructuring that lets a test schedule work between setup and the loop.
2. **`book2/ch5: net: extract ping_once_result() into a net module`** —
   move ping orchestration `cui.rs → net.rs`; `run_cmd_ping` keeps the same
   behavior, now calling `net::ping_once_result`.
3. **`book2/ch5: test: add ping_to_gateway integration test`** —
   `Cargo.toml` `[[test]]` + `tests/ping_to_gateway.rs`. At this commit
   `cargo test` runs it (network exists) and it passes.

Each commit must pass `scripts/check.sh` (fmt + clippy `-Dwarnings` + build
+ `cargo test`).

## Resolved decisions (this revision)

- **(a) Ping-core placement → new `src/net.rs`** (cohesion: keep `icmp.rs`
  a pure packet module; `net.rs` owns cross-layer operations). *Noted that
  the reviewer leaned toward `icmp.rs`; net.rs is the cohesion-driven
  recommendation — final nod requested.*
- **(b) Chapter labels → all three commits `book2/ch5:`**, appended at the
  end of ch5 as the "extend the tests" flow. Settled.
- **(c) Watchdog → include the in-OS `watchdog` task.** Confirmed there is
  no `.cargo/config.toml` / `launch_qemu.sh` timeout to rely on, and a
  shared-runner timeout would break interactive `cargo run`, so the in-OS
  watchdog is the right mechanism. Settled.
