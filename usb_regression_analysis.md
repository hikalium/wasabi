# USB Driver Regression Analysis

## Symptom

After commits `4c6e6bc` → `4ea2b89` → `7049b47` → `4dfd28a` were applied on `split-usb-20260313`,
USB stops working. The log shows repeated `address_device` timeouts:

```
[INFO] xhci: resetting port 5
[INFO] xhci: port 5 has been reset
[INFO] xhci: port 5 is enabled
[INFO] src/main.rs:72: Started to monitor serial port
[INFO] xhci.rs:1492: unhandled event: ttype=34 PortStatusChangeEvent, cc=1 (Success), slot=0
[ERROR] executor.rs:268: Future at executor.rs:271 is timed out after 1s
[INFO] xhci.rs:188: poll_ports failed: "TimedOut"
```

- `init_port()` **succeeds** (port is reset, enabled, slot assigned)
- `address_device()` **always times out** — `CommandCompletionEvent` never arrives
- Loops forever (100ms sleep → retry, but device state is `ConnectedButFailed` so no retry)

## Baseline

Working commit: **`db27e96`** (Improve USB descriptor types and add descriptor parsing helpers)

Files that differ between `db27e96` and `HEAD` (`4dfd28a`): **only `src/xhci.rs` and `src/x86.rs`**.

The `src/x86.rs` changes (Local APIC timer, MSR, interrupt handler for IRQ32) are unrelated to USB.
The USB regression is entirely within `src/xhci.rs`.

---

## Confirmed Bugs (vs `wasabi_book_dev` / `db27e96`)

### Bug 1 — `StatusStageTrb::new_out()` missing `CTRL_BIT_INTERRUPT_ON_COMPLETION`

**Location:** `src/xhci.rs` line 2500–2505

```rust
// HEAD (broken):
fn new_out() -> Self {
    Self {
        reserved: 0,
        option: 0,
        control: (TrbType::StatusStage as u32) << 10,
        //       ^^^ missing CTRL_BIT_INTERRUPT_ON_COMPLETION (bit 5)
    }
}

// wasabi_book_dev (correct):
fn new_out() -> Self {
    Self {
        reserved: 0,
        option: 0,
        control: (TrbType::StatusStage as u32) << 10
            | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_COMPLETION,
    }
}
```

**Effect:** `request_control_in_transfer()` (line 1088) uses `StatusStageTrb::new_out()` for the status
stage TRB, then waits on `status_future.await` for a `TransferEvent` on that TRB's address. Without IOC
set, the xHC never generates a `TransferEvent` for the status stage — `status_future` hangs forever.

**Impact:** All `GET_DESCRIPTOR` requests and any control OUT-status-stage transfers will hang
indefinitely once `address_device` is resolved.

**Note:** This does NOT directly explain `address_device` timing out, since `address_device` uses
`send_command()` → `CommandCompletionEvent`, not `StatusStageTrb`.

---

### Bug 2 — `StatusStageTrb::new_in()` has extra `CTRL_BIT_INTERRUPT_ON_SHORT_PACKET`

**Location:** `src/xhci.rs` line 2507–2515

```rust
// HEAD:
pub fn new_in() -> Self {
    Self {
        control: (TrbType::StatusStage as u32) << 10
            | GenericTrbEntry::CTRL_BIT_DATA_DIR_IN
            | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_COMPLETION
            | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_SHORT_PACKET,  // <-- extra
    }
}

// wasabi_book_dev:
pub fn new_in() -> Self {
    Self {
        control: (TrbType::StatusStage as u32) << 10
            | GenericTrbEntry::CTRL_BIT_DATA_DIR_IN
            | GenericTrbEntry::CTRL_BIT_INTERRUPT_ON_COMPLETION,
            // no INTERRUPT_ON_SHORT_PACKET
    }
}
```

**Effect:** INTERRUPT_ON_SHORT_PACKET on a status-stage TRB (zero-byte transfer) may cause spurious
`TransferEvent`s or double events. This was introduced in commit `7049b47`. Likely harmless for the
`address_device` timeout but may confuse the event dispatch logic.

---

### Bug 3 — `reset_port()` missing `if self.ccs()` guard

**Location:** `src/xhci.rs` line 2082–2091

```rust
// HEAD (our code):
pub async fn reset_port(&self) {
    self.assert_pp();
    while !self.pp() { yield_execution().await }
    self.assert_pr();          // always resets, even if no device
    while self.pr() { yield_execution().await }
}

// wasabi_book_dev:
pub async fn reset_port(&self) {
    self.assert_pp();
    while !self.pp() { yield_execution().await }
    if self.ccs() {            // only reset if device is connected
        self.assert_pr();
        while self.pr() { yield_execution().await }
    }
}
```

**Effect:** For a port where no device is connected, the old code would skip the PR assert. Our code
always attempts a port reset. When a device IS connected (which is required for `handle_port_connect`
to be triggered), `ccs()` is true so the behavior is identical. **Not the cause of the regression.**

---

## Primary Mystery: `address_device` Timeout

The `address_device` function (`src/xhci.rs` line 420–451) has NOT changed functionally between
`db27e96` and `4dfd28a`. Key pieces that are identical:

- `send_command()` (line 1043): push TRB → `notify_xhc()` → `EventFuture::new_for_trb()` → `.await`
- `EventFuture::new_for_trb()` (line 2190): registers waiter matching on `trb_addr`
- `EventRing::poll()` (line 1478): dispatches events to matching waiters
- The event ring poller task (line 175–180): runs in the cooperative executor, calls `poll()` and yields

The `address_device` function sends an `AddressDeviceCommand` TRB, rings the host controller doorbell,
and waits for a `CommandCompletionEvent` whose `data` field matches the TRB's physical address.

### What changed between `db27e96` and `4dfd28a` that could affect this:

#### Candidate 1 — `with_timeout` was ADDED around `address_device` (commit `4dfd28a`)

In `wasabi_book_dev` / `db27e96`, `address_device()` is called without a timeout:
```rust
let mut ctrl_ep_ring = Self::address_device(xhc, port, slot).await?;
```

In `4dfd28a`, it was wrapped with `with_timeout(1s, ...)`:
```rust
let mut ctrl_ep_ring = with_timeout(
    Duration::from_secs(1),
    Self::address_device(xhc, port, slot),
).await?;
```

`with_timeout` uses `Select2` which polls `TimeoutFuture`. `TimeoutFuture::poll()` calls:
```rust
x86::enable_interrupt();
x86::hlt();
x86::disable_interrupt();
```

This enables interrupts, halts until an interrupt, then disables interrupts again. The Local APIC
timer interrupt (IRQ32, added in commits after `db27e96`) fires and wakes up the CPU.

**Key question:** Does the `TimeoutFuture`'s `hlt()` interaction with interrupt enabling somehow
interfere with the xHCI event polling?

In the cooperative executor:
1. `Select2` polls `address_device` → `EventFuture` pending (no event yet)
2. `Select2` polls `TimeoutFuture` → enables interrupts → HLT → APIC timer fires → returns
3. `Select2` returns `Poll::Pending`
4. Executor runs next task: event ring poller → `poll()` → checks for events → yields
5. Executor runs next task: eventually back to `with_timeout` → loop

This should work correctly. BUT: if the Local APIC timer interrupt fires at the wrong time (e.g.,
between `notify_xhc()` and `EventFuture::new_for_trb()` registration... actually in a cooperative
executor there's no interleaving there since interrupts are disabled during normal execution).

Actually, when does `TimeoutFuture` call `enable_interrupt()`? It's called from `poll()`, which is
called within the cooperative executor. At this point, interrupts should be disabled (CLI). The APIC
timer interrupt (IRQ32) was added in the commits after `db27e96`:

```rust
if index == 32 {
    LocalApic::set_bsp_timer_count(10000);
    LocalApic::bsp_notify_end_of_interrupt();
    return;
}
```

This interrupt handler should be benign. But if the Local APIC timer is configured and fires
frequently, `TimeoutFuture::hlt()` will return quickly. This is fine.

#### Candidate 2 — `InputContext` moved from `Box::pin` to `IoBox` (commit `4ea2b89`)

Before:
```rust
let mut input_context = Box::pin(InputContext::default());  // regular heap (cached)
```
After:
```rust
let mut input_context = InputContext::default();  // stack
let input_context = IoBox::new(input_context);    // heap, cache-disabled
```

The `IoBox::new()` calls `disable_cache()` which remaps the memory pages to `PageAttr::ReadWriteIo`
(uncacheable via page table). The physical address passed to the xHC should be identical.

In QEMU, the xHC reads the `InputContext` from guest physical memory. Since QEMU is software
emulation, cache attributes shouldn't affect what the xHC sees. However, if `disable_cache` has a bug
(e.g., maps the wrong pages or corrupts the TLB), the data written to `input_context` before
`IoBox::new()` could be inaccessible or corrupted.

**This is the most suspicious candidate.** Specifically:
- `input_context` is built up on the stack with `Default::default()` + various `set_*` calls
- Then `IoBox::new(input_context)` **moves** the data to heap and calls `disable_cache`
- If `disable_cache` remaps the page but the CPU cache still has a stale copy, the xHC might see
  zeros (or stale data) when it reads the InputContext

However, `disable_cache` in `x86.rs` calls `create_mapping` with `PageAttr::ReadWriteIo`, which
should flush the TLB. On x86-64, cache lines are invalidated via `WBINVD` or `CLFLUSH`, not just
page table changes. If the data is in cache and the page is remapped to uncacheable, the cache
contents may be visible until evicted. In QEMU, this shouldn't matter (no real CPU cache). On real
hardware, the `disable_cache` approach could be racy.

#### Candidate 3 — `EndpointContext` fields changed to `Volatile<T>` (commit `4ea2b89`)

The `EndpointContext` struct changed `data: [u32; 2]` to `data: [Volatile<u32>; 2]` and similar for
other fields. `Volatile<T>` is `#[repr(transparent)]` so the memory layout is identical.

However, the `set_*` methods now use `.write()` / `.read()` instead of direct field access. This
should be equivalent.

#### Candidate 4 — `set_interval` mask bug in `wasabi_book_dev` vs our code

```rust
// wasabi_book_dev (buggy mask):
d &= 0xff << 16;   // should be !(0xff << 16) — clears ALL other bits, not just interval

// Our code (correct):
d &= !(0xff << 16);   // correctly preserves other bits
```

Our code actually has the CORRECT logic here. The `wasabi_book_dev` has a bug here. So this is not
the regression cause.

---

## What To Check Next

1. **Bisect between `db27e96` (working) and `4c6e6bc` (first change)**:
   Check if the regression exists at `4c6e6bc` (purely additive: inspection methods, port state types).
   If yes, something subtle in the `EventRing::poll()` "unhandled event" logging loop is affecting it.
   If no, the regression is in `4ea2b89` or later.

2. **Add logging to `address_device`** to see:
   - Is `cmd_enable_slot` completing (slot != 0)?
   - Is `send_command(cmd_address_device)` even reaching `notify_xhc()`?
   - Is `EventFuture` being polled at all?
   - Is any `CommandCompletionEvent` arriving that goes unmatched?

3. **Check if `CommandCompletionEvent` is generated but discarded**:
   Add temporary logging to `EventRing::poll()` to print ALL events, not just unhandled ones.
   If a `CommandCompletionEvent` appears in the log with the right TRB address, the waiter matching
   is broken. If no `CommandCompletionEvent` appears, the xHC is not executing the command.

4. **Verify the `with_timeout` + `TimeoutFuture::hlt()` interaction**:
   If the Local APIC timer is NOT set up when running on QEMU with `cargo test`, `TimeoutFuture`
   may `hlt` indefinitely (no interrupt to wake it). This would mean `address_device`'s `Select2`
   is stuck in HLT, never giving the event ring poller a chance to run.
   - Check: does `Impl local APIC` / `Impl local APIC timer logic` enable the APIC timer?
   - If the APIC timer is only initialized on real hardware (not QEMU), `hlt()` may block forever.

5. **Fix Bug 1 immediately** (`StatusStageTrb::new_out()` missing IOC):
   Even if it doesn't fix `address_device`, it will definitely prevent descriptor requests from working.

---

## Summary Table

| # | Location | Description | Confirmed Cause of `address_device` timeout? |
|---|----------|-------------|----------------------------------------------|
| 1 | `StatusStageTrb::new_out()` line 2504 | Missing `CTRL_BIT_INTERRUPT_ON_COMPLETION` | **No** (affects descriptor requests, not address_device) |
| 2 | `StatusStageTrb::new_in()` line 2514 | Extra `CTRL_BIT_INTERRUPT_ON_SHORT_PACKET` | No |
| 3 | `reset_port()` line 2082 | Missing `if self.ccs()` guard | No |
| 4 | `with_timeout` + `TimeoutFuture::hlt()` | May block if APIC timer not running | **Suspected** (most likely) |
| 5 | `IoBox::new(input_context)` vs `Box::pin` | Cache-disable may cause stale data on real HW | Suspected (QEMU should be fine) |

The most likely root cause for QEMU is **Candidate 4**: `TimeoutFuture::hlt()` in the `Select2`
inside `with_timeout(1s, address_device(...))` blocks the CPU waiting for an interrupt. If the Local
APIC timer is not configured (or not running) in QEMU's test environment, the CPU stays in HLT
indefinitely, preventing the event ring poller from running and consuming the xHC's completion event.
This would explain why `address_device` never completes — the event ring is never polled.

Verify by checking: when does the Local APIC timer get initialized, and is it running before USB
port connect handling begins?
