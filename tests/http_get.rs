//! Integration test: boot the OS, wait for DHCP, fetch a page from an
//! HTTP server with `http_get`, and quit QEMU with the verdict.
//!
//! Run with `cargo test --test http_get`. The server is not on the
//! Internet: `scripts/launch_qemu.sh` tells QEMU's slirp network to hand
//! every connection to 10.0.2.100:80 to `scripts/http_test_server.sh`,
//! so the test exercises ARP, the TCP handshake, the request, the
//! response and the close without leaving the machine.
#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::time::Duration;
use wasabi::boot::run_system;
use wasabi::boot::setup_system;
use wasabi::executor::sleep;
use wasabi::executor::spawn_global;
use wasabi::hpet::global_timestamp;
use wasabi::http;
use wasabi::ip::IpV4Addr;
use wasabi::nic;
use wasabi::println;
use wasabi::qemu::exit_qemu;
use wasabi::qemu::QemuExitCode;
use wasabi::result::Result;
use wasabi::uefi::EfiHandle;
use wasabi::uefi::EfiSystemTable;

/// The address `scripts/launch_qemu.sh` forwards to the test server.
const SERVER: IpV4Addr = IpV4Addr::new([10, 0, 2, 100]);
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
    spawn_global(http_get_test());
    spawn_global(watchdog());
    run_system()
}

/// Wait for DHCP, then fetch two paths: one the server knows and one it
/// doesn't. The second request also shows that the client socket can be
/// used again after a connection has closed.
async fn http_get_test() -> Result<()> {
    let deadline = global_timestamp() + DHCP_WAIT;
    while !nic::has_ip() {
        if global_timestamp() >= deadline {
            println!("http_get: FAIL (DHCP timeout)");
            exit_qemu(QemuExitCode::Fail);
        }
        sleep(Duration::from_millis(100)).await;
    }
    for (path, status, body) in [
        ("/hello", 200, "hello from the test server\n"),
        ("/nowhere", 404, "not found\n"),
    ] {
        match http::http_get(SERVER, "10.0.2.100", path).await {
            Ok(res) if res.status == status && res.body == body.as_bytes() => {
                println!("http_get: {path} -> {status}, as expected");
            }
            Ok(res) => {
                println!(
                    "http_get: FAIL ({path}: status {}, body {:?})",
                    res.status,
                    core::str::from_utf8(&res.body),
                );
                exit_qemu(QemuExitCode::Fail);
            }
            Err(e) => {
                println!("http_get: FAIL ({path}: {e})");
                exit_qemu(QemuExitCode::Fail);
            }
        }
    }
    println!("http_get: PASS");
    exit_qemu(QemuExitCode::Success);
}

/// Safety net: if no verdict is reached in time (e.g. boot stalls), fail
/// rather than letting QEMU run forever and hang `cargo test`.
async fn watchdog() -> Result<()> {
    sleep(WATCHDOG).await;
    println!("http_get: FAIL (watchdog timeout)");
    exit_qemu(QemuExitCode::Fail);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    wasabi::print::panic_print(format_args!("[http_get] PANIC: {info:?}\n"));
    exit_qemu(QemuExitCode::Fail);
}
