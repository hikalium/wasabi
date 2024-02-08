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
use core::arch::global_asm;

// https://doc.rust-lang.org/reference/items/external-blocks.html#abi
// https://doc.rust-lang.org/reference/inline-assembly.html
// https://www.intel.com/content/dam/develop/public/us/en/documents/325383-sdm-vol-2abcd.pdf#page=1401
global_asm!(
    // **** Symbols from Rust code
    ".global arch_syscall_handler",
    // **** Implementations
    ".global asm_syscall_handler",
    "asm_syscall_handler:",
    // On syscall entry,
    // RCX <= RIP to continue execution
    // RIP <= IA32_LSTAR
    // R11 <= RFLAGS
    // RFLAGS &= IA32_FMASK
    // CS <= {
    //      Selector: IA32_STAR[32..48] & 0xFFFC
    //      Base: 0
    //      Attr: 64-bit code, accessed
    //      DPL: 0
    // }
    // SS <= {
    //      Selector: IA32_STAR[32..48] + 8
    //      Base: 0
    //      Attr: RW data, accessed
    //      DPL: 0
    // }

    // Preserve registers after syscall
    "sub rsp,64",
    "push rsp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push 0", // r11 (destroyed)
    "push r10",
    "push r9",
    "push r8",
    "push rdi",
    "push rsi",
    "push rbp",
    "push rbx",
    "push rdx",
    "push 0", // rcx (destroyed)
    "push rax",
    "push r11",     // RFLAGS saved on syscall
    "push rcx",     // RIP saved on syscall
    "sub rsp, 512", // Pseudo-FpuContext
    //
    "mov rbp, rsp", // Save rsp to restore later
    "mov rdi, rsp", // First argument for syscall_handler (regs)
    "and rsp, -16", // Align the stack (to satisfy sysv64 ABI)
    "call arch_syscall_handler",
    "mov rsp, rbp", // Recover original stack value
    //
    ".global return_to_app",
    "return_to_app:",
    // Restore registers to sysret
    // This block assumes:
    // - RSP = User stack, with saved registers
    "add rsp, 512", // Pseudo-FpuContext
    "pop rcx",      // RIP saved on syscall
    "pop r11",      // RFLAGS saved on syscall
    "pop rax",
    "add rsp, 8", // rcx (destroyed)
    "pop rdx",
    "pop rbx",
    "pop rbp",
    "pop rsi",
    "pop rdi",
    "pop r8",
    "pop r9",
    "pop r10",
    "add rsp, 8", // r11 (destroyed)
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "add rsp,8", // rsp (skip)
    "add rsp,64",
    //
    "sysretq",
    // sysretq will do:
    // RIP <= RCX
    // RFLAGS <=  (R11 & 0x3C7FD7) | 2      // Clear RF, VM, reserved bits; set bit 1
    // CS <= {
    //      Selector: (IA32_STAR[48..64] + 16) | 3      // RPL = 3
    //      Base: 0
    //      Attr: 64-bit code, accessed
    //      DPL: 3
    // }
    // SS <= {
    //      Selector: (IA32_STAR[48..64] + 8) | 3       // RPL = 3
    //      Base: 0
    //      Attr: RW data, accessed
    //      DPL: 3
    // }
);
extern "C" {
    pub fn asm_syscall_handler();
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
