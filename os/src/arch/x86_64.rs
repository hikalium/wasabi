pub mod apic;
pub mod gdt;
pub mod idt;
pub mod paging;

use crate::println;
use core::arch::asm;
use core::arch::global_asm;
use core::fmt;

// Due to the syscall instruction spec
// GDT entries should be in this order:
pub const NULL_SELECTOR: u16 = 0 << 3;
pub const KERNEL_CS: u16 = 1 << 3;
pub const KERNEL_DS: u16 = 2 << 3;
pub const USER32_CS: u16 = 3 << 3;
pub const USER_DS: u16 = 4 << 3;
pub const USER64_CS: u16 = 5 << 3;

pub const MSR_IA32_APIC_BASE: u32 = 0x1b;
pub const MSR_FSB_FREQ: u32 = 0xcd;
pub const MSR_PLATFORM_INFO: u32 = 0xce;
pub const MSR_X2APIC_EOI: u32 = 0x80b;
pub const MSR_EFER: u32 = 0xC0000080;
pub const MSR_STAR: u32 = 0xC0000081;
pub const MSR_LSTAR: u32 = 0xC0000082;
pub const MSR_FMASK: u32 = 0xC0000084;
pub const MSR_FS_BASE: u32 = 0xC0000100;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC0000102;

pub fn read_rsp() -> u64 {
    let mut value;
    unsafe {
        asm!("mov rax, rsp",
            out("rax") value);
    }
    value
}

pub fn clflush(ptr: usize) {
    unsafe {
        asm!("clflush [rax]",
            in("rax") ptr,)
    }
}

/// # Safety
/// Switching rsp to another value can break execution.
/// Programmer should provide valid rsp value as new_rsp.
/// Note: we cannot return to the caller function so this
/// function is marked as never `!`.
pub unsafe fn switch_rsp(new_rsp: u64, jump_after: fn() -> !) -> ! {
    asm!("mov rsp, rax",
            "jmp rcx",
            in("rax") new_rsp,
            in("rcx") jump_after);
    unreachable!()
}

/// # Safety
/// rdmsr will cause #GP(0) if the specified MSR is not implemented.
pub unsafe fn read_msr(msr: u32) -> u64 {
    let mut high: u32;
    let mut low: u32;
    asm!("rdmsr",
            in("ecx") msr,
            out("edx") high,
            out("eax") low);
    ((high as u64) << 32) | low as u64
}

/// # Safety
/// wrmsr will cause #GP(0) if the specified MSR is not implemented,
/// reserved fields have non-zero values and
/// non-canonical address values are being set to address fields.
pub unsafe fn write_msr(port: u32, data: u64) {
    asm!("wrmsr",
            in("ecx") port,
            in("edx") (data >> 32),
            in("eax") data as u32);
}

/// # Safety
/// Anything can happen if the given selector is invalid.
pub unsafe fn write_es(selector: u16) {
    asm!(
	"mov es, ax",
                in("ax") selector)
}
/// # Safety
/// Anything can happen if the CS given is invalid.
pub unsafe fn write_cs(cs: u16) {
    // The MOV instruction CANNOT be used to load the CS register.
    // Use far-jump(ljmp) instead.
    asm!(
	"lea rax, [rip + 1f]", // Target address (label 1 below)
	"push cx", // Construct a far pointer on the stack
	"push rax",
	"ljmp [rsp]",
        "1:",
        "add rsp, 8 + 2", // Cleanup the far pointer on the stack
                in("cx") cs)
}
/// # Safety
/// Anything can happen if the given selector is invalid.
pub unsafe fn write_ss(selector: u16) {
    asm!(
	"mov ss, ax",
                in("ax") selector)
}
/// # Safety
/// Anything can happen if the given selector is invalid.
pub unsafe fn write_ds(ds: u16) {
    asm!(
	"mov ds, ax",
                in("ax") ds)
}
/// # Safety
/// Anything can happen if the given selector is invalid.
pub unsafe fn write_fs(selector: u16) {
    asm!(
	"mov fs, ax",
                in("ax") selector)
}
/// # Safety
/// Anything can happen if the given selector is invalid.
pub unsafe fn write_gs(selector: u16) {
    asm!(
	"mov gs, ax",
                in("ax") selector)
}

pub fn busy_loop_hint() {
    unsafe { asm!("pause") }
}

#[derive(Copy, Clone)]
pub struct CpuidRequest {
    pub eax: u32,
    pub ecx: u32,
}
impl fmt::Display for CpuidRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CpuidRequest(EAX: {:#010X}, ECX:{:#010X})",
            self.eax, self.ecx,
        )
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct CpuidResponse {
    // Do not reorder the members!
    // This order matches the internal
    // encodings of the registers.
    ebx: u32,
    edx: u32,
    ecx: u32,
    eax: u32,
}
impl CpuidResponse {
    pub fn eax(&self) -> u32 {
        self.eax
    }
    pub fn ebx(&self) -> u32 {
        self.ebx
    }
    pub fn ecx(&self) -> u32 {
        self.ecx
    }
    pub fn edx(&self) -> u32 {
        self.edx
    }
}
impl fmt::Display for CpuidResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "CpuidResponse(EAX: {:#010X}, EBX:{:#010X}, ECX:{:#010X}, EDX:{:#010X})",
            self.eax, self.ebx, self.ecx, self.edx
        )
    }
}

// Returned values will be all zero if the requested leaf
// does not exist.
pub fn read_cpuid(request: CpuidRequest) -> CpuidResponse {
    let mut eax: u32 = request.eax;
    let mut ebx: u32;
    let mut ecx: u32 = request.ecx;
    let mut edx: u32;
    unsafe {
        asm!(
            "xchg rsi,rbx",
            "cpuid",
            "xchg rsi,rbx",
            inout("eax") eax,
            out("esi") ebx,
            inout("ecx") ecx,
            out("edx") edx,
            clobber_abi("C"),
        );
    }
    CpuidResponse { eax, ebx, ecx, edx }
}

pub fn write_io_port_u8(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al",
            in("al") data,
            in("dx") port)
    }
}
pub fn write_io_port_u16(port: u16, data: u16) {
    unsafe {
        asm!("out dx, ax",
            in("ax") data,
            in("dx") port)
    }
}
pub fn write_io_port_u32(port: u16, data: u32) {
    unsafe {
        asm!("out dx, eax",
            in("eax") data,
            in("dx") port)
    }
}

pub fn read_io_port_u8(port: u16) -> u8 {
    let mut data: u8;
    unsafe {
        asm!("in al, dx",
            out("al") data,
            in("dx") port)
    }
    data
}
pub fn read_io_port_u16(port: u16) -> u16 {
    let mut data: u16;
    unsafe {
        asm!("in ax, dx",
            out("ax") data,
            in("dx") port)
    }
    data
}
pub fn read_io_port_u32(port: u16) -> u32 {
    let mut data: u32;
    unsafe {
        asm!("in eax, dx",
            out("eax") data,
            in("dx") port)
    }
    data
}

pub fn disable_legacy_pic() {
    // https://wiki.osdev.org/8259_PIC#Disabling
    write_io_port_u8(0xa1, 0xff);
    write_io_port_u8(0x21, 0xff);
    println!("Disabled legacy PIC");
}

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn stihlt() {
    unsafe { asm!("sti; hlt") }
}

pub fn rest_in_peace() -> ! {
    unsafe {
        loop {
            core::arch::asm!("cli;hlt;")
        }
    }
}

// https://doc.rust-lang.org/reference/items/external-blocks.html#abi
// https://doc.rust-lang.org/reference/inline-assembly.html
// https://www.intel.com/content/dam/develop/public/us/en/documents/325383-sdm-vol-2abcd.pdf#page=1401

global_asm!(
    ".global syscall_handler",
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
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    //
    "push r9",
    "push r8",
    "push r10",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rax",
    //
    "mov rbp, rsp",
    "mov rdi, rsp",
    "and rsp, -16",
    "call syscall_handler",
    "mov rsp, rbp // Revert to user stack",
    //
    "pop rax",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop r10",
    "pop r8",
    "pop r9",
    //
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbp",
    "pop rbx",
    //
    "pop r11 // Saved RFLAGS",
    "pop rcx // Saved RIP",
    //
    "sysretq",
    // sysretq will do:
    // RIP <= RCX
    // RIP <= IA32_LSTAR
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
    let star = (KERNEL_CS as u64) << 32 | (USER64_CS as u64) << 48;
    // SAFETY: This is safe since we believe we provide appropriate star value.
    unsafe {
        write_msr(MSR_STAR, star);
        write_msr(MSR_LSTAR, asm_syscall_handler as *const () as u64);
        write_msr(MSR_FMASK, 1u64 << 9);
        let mut efer = read_msr(MSR_EFER);
        efer |= 1; // SCE: System Call Enable
        write_msr(MSR_EFER, efer);
    }
}
