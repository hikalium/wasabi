use crate::boot_info::BootInfo;
use crate::println;
use core::arch::asm;
use core::arch::global_asm;
use core::cell::RefCell;
use core::fmt;
use core::mem::size_of;
use core::mem::size_of_val;
use core::mem::MaybeUninit;

// System V AMD64 (sysv64) ABI:
//   args: RDI, RSI, RDX, RCX, R8, R9
//   callee-saved: RBX, RBP, R12, R13, R14, R15
//   caller-saved: otherwise

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct FPUContenxt {
    data: [u8; 512],
}
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct GeneralRegisterContext {
    rax: u64,
    rdx: u64,
    rbx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rcx: u64,
}
const _: () = assert!(size_of::<GeneralRegisterContext>() == (16 - 1) * 8);
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct InterruptContext {
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}
const _: () = assert!(size_of::<InterruptContext>() == 8 * 5);
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct InterruptInfo {
    // This struct is placed at top of the interrupt stack.
    fpu_context: FPUContenxt, // used by FXSAVE / FXRSTOR
    _dummy: u64,
    greg: GeneralRegisterContext,
    error_code: u64,
    ctx: InterruptContext,
}
const _: () = assert!(size_of::<InterruptInfo>() == (16 + 4 + 1) * 8 + 8 + 512);
impl fmt::Debug for InterruptInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "
        {{
            rip: {:#018X}, CS: {:#06X},
            rsp: {:#018X}, SS: {:#06X},
            rbp: {:#018X},

            rflags:     {:#018X},
            error_code: {:#018X},

            rax: {:#018X}, rcx: {:#018X},
            rdx: {:#018X}, rbx: {:#018X},
            rsi: {:#018X}, rdi: {:#018X},
            r8:  {:#018X}, r9:  {:#018X},
            r10: {:#018X}, r11: {:#018X},
            r12: {:#018X}, r13: {:#018X},
            r14: {:#018X}, r15: {:#018X},
        }}",
            self.ctx.rip,
            self.ctx.cs,
            self.ctx.rsp,
            self.ctx.ss,
            self.greg.rbp,
            self.ctx.rflags,
            self.error_code,
            //
            self.greg.rax,
            self.greg.rcx,
            self.greg.rdx,
            self.greg.rbx,
            //
            self.greg.rsi,
            self.greg.rdi,
            //
            self.greg.r8,
            self.greg.r9,
            self.greg.r10,
            self.greg.r11,
            self.greg.r12,
            self.greg.r13,
            self.greg.r14,
            self.greg.r15,
        )
    }
}

// SDM Vol.3: 6.14.2 64-Bit Mode Stack Frame
// In IA-32e mode, the RSP is aligned to a 16-byte boundary
// before pushing the stack frame

/// This generates interrupt_entrypointN()
/// Generated asm will be looks like this:
/// ```
/// .global interrupt_entrypointN
///    interrupt_entrypointN:
///    push 0 // No error code
///    push rcx // Save rcx first to reuse
///    mov rcx, N // INT#
///    jmp inthandler_common
/// ```
macro_rules! interrupt_entrypoint {
    ($index:literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
            "push 0 // No error code\n",
            "push rcx // Save rcx first to reuse\n",
            "mov rcx, ",
            stringify!($index),
            "\n",
            "jmp inthandler_common"
        ));
    };
}

interrupt_entrypoint!(3);
interrupt_entrypoint!(6);
interrupt_entrypoint!(13);
interrupt_entrypoint!(14);
interrupt_entrypoint!(32);

extern "sysv64" {
    fn interrupt_entrypoint3();
    fn interrupt_entrypoint6();
    fn interrupt_entrypoint13();
    fn interrupt_entrypoint14();
    fn interrupt_entrypoint32();
}

global_asm!(
    r#"
.global inthandler_common
inthandler_common:
    // General purpose registers (except rsp and rcx)
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rbx
    push rdx
    push rax
    // FPU State
    sub rsp, 512 + 8
    fxsave64[rsp]
    // 1st parameter: pointer to the saved CPU state
    mov rdi, rsp
    // Align the stack to 16-bytes boundary
    mov rbp, rsp
    and rsp, -16
    // 2nd parameter: Int#
    mov rsi, rcx

    call inthandler

    mov rsp, rbp
    //
    fxrstor64[rsp]
    add rsp, 512 + 8
    //
    pop rax
    pop rdx
    pop rbx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    //
	pop rcx
	add rsp, 8 // for Error Code
	iretq

"#
);

#[no_mangle]
extern "sysv64" fn inthandler(info: &InterruptInfo, index: usize) {
    // Silent
    if index == 32 {
        let bsp_local_apic = BootInfo::take().bsp_local_apic();
        bsp_local_apic.notify_end_of_interrupt();
        return;
    }
    // Informational
    println!("Interrupt Info: {:?}", info);
    if index == 3 {
        println!("Exception {index:#04X}: Breakpoint");
        return;
    }
    // Fatal
    if index == 6 {
        println!("Exception {index:#04X}: Invalid Opcode");
    } else {
        println!("Exception {index:#04X}: Not handled");
    }
    panic!();
}

#[no_mangle]
extern "sysv64" fn int_handler_unimplemented() {
    panic!("unexpected interrupt!");
}

mod attr_bits {
    // PDDRTTTT (TTTT: type, R: reserved, D: DPL, P: present)
    pub const BIT_FLAGS_INTGATE: u8 = 0b0000_1110u8;
    pub const BIT_FLAGS_PRESENT: u8 = 0b1000_0000u8;
}
use attr_bits::{BIT_FLAGS_INTGATE, BIT_FLAGS_PRESENT};

#[repr(u8)]
enum IdtAttr {
    // Without _NotPresent value, MaybeUninit::zeroed() on
    // this struct will be undefined behavior.
    _NotPresent = 0,
    IntGateDPL0 = BIT_FLAGS_INTGATE | BIT_FLAGS_PRESENT,
}

#[repr(packed)]
#[allow(dead_code)]
pub struct IdtDescriptor {
    offset_low: u16,
    segment_selector: u16,
    ist_index: u8,
    attr: IdtAttr,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}
const _: () = assert!(size_of::<IdtDescriptor>() == 16);
impl IdtDescriptor {
    fn new(
        segment_selector: u16,
        ist_index: u8,
        attr: IdtAttr,
        f: unsafe extern "sysv64" fn(),
    ) -> Self {
        let handler_addr = f as *const unsafe extern "sysv64" fn() as usize;
        Self {
            offset_low: handler_addr as u16,
            offset_mid: (handler_addr >> 16) as u16,
            offset_high: (handler_addr >> 32) as u32,
            segment_selector,
            ist_index,
            attr,
            _reserved: 0,
        }
    }
}

#[allow(dead_code)]
#[repr(packed)]
struct IdtrParameters<'a> {
    limit: u16,
    base: &'a [IdtDescriptor; 0x100],
}

pub struct Idt {
    entries: RefCell<[IdtDescriptor; 0x100]>,
}
impl Idt {
    const fn new() -> Self {
        // This is safe since it does not contain any references
        // and it will be valid IDT with non-present entries.
        Self {
            entries: RefCell::new(unsafe { MaybeUninit::zeroed().assume_init() }),
        }
    }
    pub fn init(&self, segment_selector: u16) {
        let entries = &mut *self.entries.borrow_mut();
        for e in entries.iter_mut() {
            *e = IdtDescriptor::new(
                segment_selector,
                0,
                IdtAttr::IntGateDPL0,
                int_handler_unimplemented,
            );
        }
        entries[3] = IdtDescriptor::new(
            segment_selector,
            0,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint3,
        );
        entries[6] = IdtDescriptor::new(
            segment_selector,
            0,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint6,
        );
        entries[13] = IdtDescriptor::new(
            segment_selector,
            0,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint13,
        );
        entries[14] = IdtDescriptor::new(
            segment_selector,
            0,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint14,
        );
        entries[32] = IdtDescriptor::new(
            segment_selector,
            0,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint32,
        );
    }
    /// # Safety
    /// It is programmer's responsibility to call this method
    /// with a valid, correct IDT.
    pub unsafe fn load(&'static self) {
        let entries = &*self.entries.borrow();
        let params = IdtrParameters {
            limit: (size_of_val(entries) - 1) as u16,
            base: entries,
        };
        asm!("lidt [rcx]",
                in("rcx") &params);
        println!("LDT @ {:#p} loaded.", entries);
    }
}
// This impl is safe as far as the OS is running in a single thread.
unsafe impl Sync for Idt {}

pub static IDT: Idt = Idt::new();
