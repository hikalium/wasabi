// Based on: https://darkcoding.net/software/a-very-small-rust-binary-indeed/
#![no_std]
#![no_main]
use core::arch::asm;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        asm!(
            "mov edi, 42",
            "mov eax, 60",
            "syscall",
            options(nostack, noreturn)
        )
        // nostack prevents `asm!` from push/pop rax
        // noreturn prevents it putting a 'ret' at the end
        //  but it does put a ud2 (undefined instruction) instead
    }
}

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
