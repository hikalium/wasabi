//! Integration test: boot the OS, wait for DHCP, ping the default gateway,
//! and quit QEMU with the verdict.
//!
//! Run with `cargo test --test ping_to_gateway`. It uses the same QEMU
//! runner as `cargo run`/`cargo test` (`scripts/launch_qemu.sh`), which
//! attaches slirp networking (gateway 10.0.2.2) and `isa-debug-exit`, and
//! turns `exit_qemu(Success)` (QEMU status 3) into a passing test and
//! `exit_qemu(Fail)` (status 5) / a panic into a failing one.
//!
//! Unlike a `#[test_case]` unit test (which runs under a minimal boot with
//! no network), this needs the full boot, so it schedules its work between
//! `setup_system()` and `run_system()`.
#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::time::Duration;
use wasabi::boot::run_system;
use wasabi::boot::setup_system;
use wasabi::executor::sleep;
use wasabi::executor::spawn_global;
use wasabi::hpet::global_timestamp;
use wasabi::net;
use wasabi::nic;
use wasabi::println;
use wasabi::qemu::exit_qemu;
use wasabi::qemu::QemuExitCode;
use wasabi::result::Result;
use wasabi::uefi::EfiHandle;
use wasabi::uefi::EfiSystemTable;

/// Number of echo requests to try before declaring failure.
const PING_COUNT: u16 = 4;
/// How long to wait for DHCP to hand us an IP and a default router.
const DHCP_WAIT: Duration = Duration::from_secs(10);
/// Absolute upper bound on the whole test, in case something stalls before
/// a verdict is produced.
const WATCHDOG: Duration = Duration::from_secs(30);

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    // Hold the GDT/IDT for the life of the OS (see boot::setup_system docs):
    // run_system() never returns, so this named binding lives forever.
    let _descriptor_tables = setup_system(image_handle, efi_system_table);
    spawn_global(ping_gateway_test());
    spawn_global(watchdog());
    run_system()
}

/// Wait for DHCP, then ping the learned default gateway. Quits QEMU with
/// Success on the first reply, Fail if DHCP never completes or no reply
/// arrives.
async fn ping_gateway_test() -> Result<()> {
    let deadline = global_timestamp() + DHCP_WAIT;
    loop {
        if nic::has_ip() && nic::router().is_some() {
            break;
        }
        if global_timestamp() >= deadline {
            println!(
                "ping_to_gateway: FAIL (DHCP timeout; ip={}, gw={:?})",
                nic::has_ip(),
                nic::router(),
            );
            exit_qemu(QemuExitCode::Fail);
        }
        sleep(Duration::from_millis(100)).await;
    }

    let gw = nic::router().expect("router is set");
    println!("ping_to_gateway: pinging default gateway {gw}");
    for seq in 1..=PING_COUNT {
        match net::ping_once_result(gw, seq).await {
            Ok(Some((rtt, src))) => {
                println!(
                    "ping_to_gateway: PASS (reply from {src}, {} us)",
                    rtt.as_micros(),
                );
                exit_qemu(QemuExitCode::Success);
            }
            Ok(None) => println!("ping_to_gateway: no reply (icmp_seq={seq})"),
            Err(e) => {
                println!("ping_to_gateway: ping error (icmp_seq={seq}): {e}")
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
    println!(
        "ping_to_gateway: FAIL (no reply from {gw} after {} tries)",
        PING_COUNT
    );
    exit_qemu(QemuExitCode::Fail);
}

/// Safety net: if no verdict is reached in time (e.g. boot stalls), fail
/// rather than letting QEMU run forever and hang `cargo test`.
async fn watchdog() -> Result<()> {
    sleep(WATCHDOG).await;
    println!("ping_to_gateway: FAIL (watchdog timeout)");
    exit_qemu(QemuExitCode::Fail);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    wasabi::print::panic_print(format_args!(
        "[ping_to_gateway] PANIC: {info:?}\n"
    ));
    exit_qemu(QemuExitCode::Fail);
}
