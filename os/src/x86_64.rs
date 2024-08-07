// x86_64 specific implementations

/*

msabi: Microsoft ABI (a.k.a. __fastcall, ms_abi)
(source: https://learn.microsoft.com/en-us/cpp/build/x64-software-conventions?view=msvc-170#x64-register-usage)
    retv: rax
    func:
    argN: rcx, rdx, r8, r9
    temp: r10, r11
    keep: rbx, rsp, rbp, rsi, rdi, r12, r13, r14, r15

sysv: System V ABI
(source: [sysv_abi_0_99] Figure 3.4: Register Usage)
    retv: rax
    func:
    argN: rdi, rsi, rdx, rcx, r8, r9
    temp: r10, r11
    keep: rbx, rsp, rbp, r12, r13, r14, r15

linux: Linux syscall ABI
(source: [sysv_abi_0_99] A.2 AMD64 Linux Kernel Conventions)
    retv: rax
    func: rax
    argN: rdi, rsi, rdx, r10, r8, r9        // using r10 instead of rcx in the userland
    temp: rcx, r11                          // destroyed by the syscall instruction
    keep: rbx, rsp, rbp, r12, r13, r14, r15

wasabi: WasabiOS syscall ABI
    retv: rax
    func: rdx
    argN: rsi, rdi, r8, r9, r10
    temp: rcx, r11                          // destroyed by the syscall instruction
    keep: rbx, rsp, rbp, r12, r13, r14, r15

Comparison:
            msabi       sysv        linux       wasabi
            ----        ----        ----        ----
    rax     retv        retv        retv/func   retv
    rcx     arg1        arg4        temp        temp
    rdx     arg2        arg3        arg3        func
    rbx     keep        keep        keep        keep
    rsp     keep        keep        keep        keep
    rbp     keep        keep        keep        keep
    rsi     keep        arg2        arg2        arg1
    rdi     keep        arg1        arg1        arg2
    r8      arg3        arg5        arg5        arg3
    r9      arg4        arg6        arg6        arg4
    r10     temp        temp        arg4        arg5
    r11     temp        temp        temp        arg6
    r12     keep        keep        keep        keep
    r13     keep        keep        keep        keep
    r14     keep        keep        keep        keep
    r15     keep        keep        keep        keep
*/

pub mod apic;
pub mod context;
pub mod gdt;
pub mod idt;
pub mod paging;
pub mod syscall;

extern crate alloc;

use crate::serial::SerialPort;
use core::arch::asm;
use core::fmt;
use core::fmt::Write;
use core::slice;

// Due to the syscall instruction spec
// GDT entries should be in this order:
pub const NULL_SELECTOR: u16 = 0 << 3;
pub const KERNEL_CS: u16 = 1 << 3;
pub const KERNEL_DS: u16 = 2 << 3;
pub const USER32_CS: u16 = 3 << 3 | 0b11 /* RPL=3 */;
pub const USER_DS: u16 = 4 << 3 | 0b11 /* RPL=3 */;
pub const USER64_CS: u16 = 5 << 3 | 0b11 /* RPL=3 */;
pub const TSS64_SEL: u16 = 6 << 3;

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

pub fn read_cr2() -> u64 {
    let mut cr2: u64;
    unsafe {
        asm!("mov rax, cr2",
            out("rax") cr2)
    }
    cr2
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
}

pub fn allow_interrupts() {
    unsafe { asm!("sti") }
}

pub fn block_interrupts() {
    unsafe { asm!("cli") }
}

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn trigger_debug_interrupt() {
    unsafe { asm!("int3") }
}

pub fn stihlt() {
    unsafe { asm!("sti; hlt") }
}

pub fn rest_in_peace() -> ! {
    loop {
        unsafe { asm!("cli;hlt;") }
    }
}

#[no_mangle]
pub fn dump_stack() {
    let mut serial_writer = SerialPort::default();
    // push rbp -> rbp = initial_rsp_on_func_entry -> (func body) -> pop rbp -> ret
    let mut rbp: u64;
    let mut rsp: u64;
    let mut rip: u64;
    unsafe {
        asm!(
        "lea rdx, [rip+0f]",
        "call rdx",
        "0:",
        "pop rdx",
        "mov rax, rbp",
        "mov rcx, rsp",
        out("rax") rbp,
        out("rcx") rsp,
        out("rdx") rip,
        );
    }
    writeln!(serial_writer, "[PANIC] RIP = {rip:#018X}").unwrap();
    writeln!(serial_writer, "[PANIC] RSP = {rsp:#018X}").unwrap();
    writeln!(serial_writer, "[PANIC] RBP = {rbp:#018X}").unwrap();
    writeln!(serial_writer, "[PANIC] *RBP = {:#018X}", unsafe {
        *(rbp as *const u64)
    })
    .unwrap();
    writeln!(
        serial_writer,
        "[PANIC] dump_stack() = {:#018X}",
        dump_stack as *const fn() as u64
    )
    .unwrap();
    for i in 0..1024 {
        let addr = rsp + i * 8;
        writeln!(
            serial_writer,
            "[PANIC] *({:#018X}) : {:#018X}",
            addr,
            unsafe { *(addr as *const u64) }
        )
        .unwrap();
    }
    let stack = unsafe { slice::from_raw_parts(rbp as *const u8, 64) };
    crate::print::hexdump(stack);
}
