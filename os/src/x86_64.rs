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
pub mod gdt;
pub mod idt;
pub mod paging;
pub mod syscall;

extern crate alloc;

use crate::mutex::Mutex;
use core::arch::asm;
use core::fmt;
use core::mem::size_of;
use core::mem::MaybeUninit;

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

pub static CONTEXT_OS: Mutex<ExecutionContext> =
    Mutex::new(ExecutionContext::default(), "CONTEXT_OS");
pub static CONTEXT_APP: Mutex<ExecutionContext> =
    Mutex::new(ExecutionContext::default(), "CONTEXT_APP");

#[repr(C)]
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    pub fpu: FpuContext,
    pub cpu: CpuContext,
    // CpuContext should be at the end to put rsp at bottom
}
impl ExecutionContext {
    /// # Safety
    /// This function should only be used for passing the ptr
    /// to the context-switching asm macro so that they can
    /// access to the context without taking a lock.
    /// The context-switching code does not modify the context
    /// while the asm code is running, so the borrow rule for
    /// the object will be maintained from the Rust's point of view.
    pub unsafe fn as_mut_ptr(&mut self) -> *mut Self {
        self as *mut Self
    }
    // We implement this outside of trait since Default trait is not const yet.
    // c.f. https://github.com/rust-lang/rust/issues/67792
    pub const fn default() -> Self {
        Self {
            fpu: FpuContext { data: [0u8; 512] },
            cpu: CpuContext::default(),
        }
    }
}
const _: () =
    assert!(size_of::<ExecutionContext>() == size_of::<FpuContext>() + size_of::<CpuContext>());

#[repr(C, align(16))]
#[derive(Clone, Debug)]
pub struct FpuContext {
    // See manual for FXSAVE and FXRSTOR
    // Should be aligned on 16-byte boundary
    pub data: [u8; 512],
}
const _: () = assert!(size_of::<FpuContext>() == 512);

#[repr(C)]
#[derive(Clone, Debug)]
pub struct CpuContext {
    pub rip: u64,
    pub rflags: u64,
    //
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64, // rsp should be here to make load / store easy
}
impl CpuContext {
    // We implement this outside of trait since Default trait is not const yet.
    // c.f. https://github.com/rust-lang/rust/issues/67792
    const fn default() -> Self {
        // SAFETY: CpuContext only contains integers so zeroed out structure is completely valid as
        // this type.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
}
const _: () = assert!(size_of::<CpuContext>() == 8 * 16 + 8 * 2);

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
