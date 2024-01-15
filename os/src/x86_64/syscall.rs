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
    ".global syscall_handler",
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

    // Save all registers
    "push rcx // Saved RIP",
    "push r11 // Saved RFLAGS",
    //
    "push rbx",
    "push rbp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    //
    "push r10",
    "push r9",
    "push r8",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rax",
    //
    "mov rbp, rsp",
    "mov rdi, rsp",
    "and rsp, -16",
    "call syscall_handler",
    "mov rsp, rbp // Revert to user stack",
    //
    "pop rax",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop r8",
    "pop r9",
    "pop r10",
    //
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "pop rbp",
    "pop rbx",
    //
    "pop r11 // Saved RFLAGS",
    "pop rcx // Saved RIP",
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