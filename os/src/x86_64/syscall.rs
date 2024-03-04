//! # syscall context transitions
//!
//! ## Basic execution state
//!
//! In x86, we have following elements in the execution environment:
//!
//! ### u64 registers (in it's opcode encoding order):
//! - RAX
//! - RCX
//! - RDX
//! - RBX
//! - RSP
//! - RBP
//! - RSI
//! - RDI
//! - R8-15
//!
//! ### segment registers (in it's opcode encoding order)
//! - ES
//! - CS
//! - SS
//! - DS
//! - FS
//! - GS
//!
//! ### other
//! - RFLAGS
//! - RIP

use crate::x86_64::context::ExecutionContext;
use crate::x86_64::context::CONTEXT_APP;
use crate::x86_64::context::CONTEXT_OS;
use crate::x86_64::read_msr;
use crate::x86_64::write_msr;
use crate::x86_64::KERNEL_CS;
use crate::x86_64::MSR_EFER;
use crate::x86_64::MSR_FMASK;
use crate::x86_64::MSR_LSTAR;
use crate::x86_64::MSR_STAR;
use crate::x86_64::USER32_CS;

extern "C" {
    pub fn asm_syscall_handler(); // in os/src/x86_64/context.rs
}

pub fn init_syscall() {
    let star = (KERNEL_CS as u64) << 32 | (USER32_CS as u64) << 48;
    // SAFETY: This is safe since we believe we provide appropriate star value.
    unsafe {
        write_msr(MSR_STAR, star);
        write_msr(MSR_LSTAR, asm_syscall_handler as *const () as u64);
        write_msr(
            MSR_FMASK,
            1u64 << 9, /* RFLAGS.IF is masked (= disable interrupt on syscall) */
        );
        let mut efer = read_msr(MSR_EFER);
        efer |= 1; // SCE: System Call Enable
        write_msr(MSR_EFER, efer);
    }
}

pub fn write_return_value_to_app(retv: u64) {
    CONTEXT_APP.lock().cpu.rax = retv;
}

pub fn write_return_value(retv: u64) {
    CONTEXT_OS.lock().cpu.rax = retv;
}

pub fn write_exit_reason(retv: u64) {
    CONTEXT_OS.lock().cpu.r8 = retv;
}

pub fn return_to_os() {
    let return_to = CONTEXT_OS.lock().cpu.rip;
    // SAFETY: This is safe as far as the CONTEXT_OS is valid so that
    // we can return to the OS world correctly.
    unsafe {
        let os_ctx = CONTEXT_OS.lock().as_mut_ptr();
        let app_ctx = CONTEXT_APP.lock().as_mut_ptr();
        // c.f. https://rust-lang.github.io/unsafe-code-guidelines/layout/function-pointers.html
        let f: extern "sysv64" fn(
            *mut ExecutionContext, /* rdi */
            *mut ExecutionContext, /* rsi */
        ) -> ! = core::mem::transmute(return_to);
        f(os_ctx, app_ctx)
    }
}

#[no_mangle]
pub extern "sysv64" fn arch_syscall_handler(ctx: &mut ExecutionContext) {
    //wasabi: WasabiOS syscall ABI
    //    retv: rax
    //    func: rdx
    //    argN: rsi, rdi, r8, r9, r10
    //    temp: rcx, r11                          // destroyed by the syscall instruction
    //    keep: rbx, rsp, rbp, r12, r13, r14, r15
    {
        // Save the app context
        let mut app_ctx = CONTEXT_APP.lock();
        *app_ctx = ctx.clone();
    }
    let args = [
        ctx.cpu.rsi,
        ctx.cpu.rdi,
        ctx.cpu.r8,
        ctx.cpu.r9,
        ctx.cpu.r10,
    ];
    let op = ctx.cpu.rdx;
    let ret = crate::syscall::syscall_handler(op, &args);
    ctx.cpu.rax = ret;
}
