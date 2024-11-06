extern crate alloc;

use crate::error;
use crate::info;
use crate::mmio::IoBox;
use crate::result::Result;
use alloc::boxed::Box;
use core::arch::asm;
use core::arch::global_asm;
use core::fmt;
use core::marker::PhantomData;
use core::mem::offset_of;
use core::mem::ManuallyDrop;
use core::mem::MaybeUninit;
use core::pin::Pin;

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn busy_loop_hint() {
    unsafe { asm!("pause") }
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
pub fn write_io_port_u8(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al",
            in("al") data,
            in("dx") port)
    }
}

pub const PAGE_SIZE: usize = 4096;
const ATTR_MASK: u64 = 0xFFF;
const ATTR_PRESENT: u64 = 1 << 0;
const ATTR_WRITABLE: u64 = 1 << 1;
const ATTR_USER: u64 = 1 << 2;
const ATTR_WRITE_THROUGH: u64 = 1 << 3;
const ATTR_CACHE_DISABLE: u64 = 1 << 4;

#[derive(Debug, Copy, Clone)]
#[repr(u64)]
pub enum PageAttr {
    NotPresent = 0,
    ReadWriteKernel = ATTR_PRESENT | ATTR_WRITABLE,
    ReadWriteUser = ATTR_PRESENT | ATTR_WRITABLE | ATTR_USER,
    ReadWriteIo = ATTR_PRESENT | ATTR_WRITABLE | ATTR_WRITE_THROUGH | ATTR_CACHE_DISABLE,
}
#[derive(Debug, Eq, PartialEq)]
pub enum TranslationResult {
    PageMapped4K { phys: u64 },
    PageMapped2M { phys: u64 },
    PageMapped1G { phys: u64 },
}

#[repr(transparent)]
pub struct Entry<const LEVEL: usize, NEXT> {
    value: u64,
    next_type: PhantomData<NEXT>,
}
impl<const LEVEL: usize, NEXT> Entry<LEVEL, NEXT> {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn is_present(&self) -> bool {
        (self.read_value() & (1 << 0)) != 0
    }
    fn is_writable(&self) -> bool {
        (self.read_value() & (1 << 1)) != 0
    }
    fn is_user(&self) -> bool {
        (self.read_value() & (1 << 2)) != 0
    }
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "L{}Entry @ {:#p} {{ {:#018X} {}{}{} ",
            LEVEL,
            self,
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" }
        )?;
        write!(f, " }}")
    }
    fn table(&self) -> Result<&NEXT> {
        if self.is_present() {
            Ok(unsafe { &*((self.value & !ATTR_MASK) as *const NEXT) })
        } else {
            Err("Page Not Found")
        }
    }
    fn table_mut(&mut self) -> Result<&mut NEXT> {
        if self.is_present() {
            Ok(unsafe { &mut *((self.value & !ATTR_MASK) as *mut NEXT) })
        } else {
            Err("Page Not Found")
        }
    }
    fn page(&self) -> Result<u64> {
        if self.is_present() {
            Ok(self.value & !ATTR_MASK)
        } else {
            Err("Page Not Found")
        }
    }
    fn populate(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Err("Page is already populated")
        } else {
            let next: Box<NEXT> = Box::new(unsafe { MaybeUninit::zeroed().assume_init() });
            self.value = Box::into_raw(next) as u64 | PageAttr::ReadWriteUser as u64;
            Ok(self)
        }
    }
    fn ensure_populated(&mut self) -> Result<&mut Self> {
        if self.is_present() {
            Ok(self)
        } else {
            self.populate()
        }
    }
    fn set_page(&mut self, phys: u64, attr: PageAttr) -> Result<()> {
        if phys & ATTR_MASK != 0 {
            Err("phys is not aligned")
        } else {
            self.value = phys | attr as u64;
            Ok(())
        }
    }
}
impl<const LEVEL: usize, NEXT> fmt::Display for Entry<LEVEL, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
impl<const LEVEL: usize, NEXT> fmt::Debug for Entry<LEVEL, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[repr(align(4096))]
pub struct Table<const LEVEL: usize, NEXT> {
    entry: [Entry<LEVEL, NEXT>; 512],
}
impl<const LEVEL: usize, NEXT: core::fmt::Debug> Table<LEVEL, NEXT> {
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "L{}Table @ {:#p} {{", LEVEL, self)?;
        for i in 0..512 {
            let e = &self.entry[i];
            if !e.is_present() {
                continue;
            }
            writeln!(f, "  entry[{:3}] = {:?}", i, e)?;
        }
        writeln!(f, "}}")
    }
    const fn index_shift() -> usize {
        (LEVEL - 1) * 9 + 12
    }
    fn calc_index(&self, addr: u64) -> usize {
        ((addr >> Self::index_shift()) & 0b1_1111_1111) as usize
    }
    pub fn next_level(&self, index: usize) -> Option<&NEXT> {
        self.entry.get(index).and_then(|e| e.table().ok())
    }
}
impl<const LEVEL: usize, NEXT: fmt::Debug> fmt::Debug for Table<LEVEL, NEXT> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

pub type PT = Table<1, [u8; PAGE_SIZE]>;
pub type PD = Table<2, PT>;
pub type PDPT = Table<3, PD>;
pub type PML4 = Table<4, PDPT>;

impl PML4 {
    pub fn new() -> Box<Self> {
        Box::new(Self::default())
    }
    fn default() -> Self {
        // This is safe since entries filled with 0 is valid.
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    pub fn create_mapping(
        &mut self,
        virt_start: u64,
        virt_end: u64,
        phys: u64,
        attr: PageAttr,
    ) -> Result<()> {
        let table = self;
        let mut addr = virt_start;
        loop {
            let index = table.calc_index(addr);
            let table = table.entry[index].ensure_populated()?.table_mut()?;
            loop {
                let index = table.calc_index(addr);
                let table = table.entry[index].ensure_populated()?.table_mut()?;
                loop {
                    let index = table.calc_index(addr);
                    let table = table.entry[index].ensure_populated()?.table_mut()?;
                    loop {
                        let index = table.calc_index(addr);
                        let pte = &mut table.entry[index];
                        let phys_addr = phys + addr - virt_start;
                        pte.set_page(phys_addr, attr)?;
                        addr = addr.wrapping_add(PAGE_SIZE as u64);
                        if index + 1 >= (1 << 9) || addr >= virt_end {
                            break;
                        }
                    }
                    if index + 1 >= (1 << 9) || addr >= virt_end {
                        break;
                    }
                }
                if index + 1 >= (1 << 9) || addr >= virt_end {
                    break;
                }
            }
            if index + 1 >= (1 << 9) || addr >= virt_end {
                break;
            }
        }
        Ok(())
    }
    pub fn translate(&self, virt: u64) -> Result<TranslationResult> {
        let index = self.calc_index(virt);
        let entry = &self.entry[index];
        let table = entry.table()?;

        let index = table.calc_index(virt);
        let entry = &table.entry[index];
        let table = entry.table()?;

        let index = table.calc_index(virt);
        let entry = &table.entry[index];
        let table = entry.table()?;

        let index = table.calc_index(virt);
        let entry = &table.entry[index];
        let page = entry.page()?;

        Ok(TranslationResult::PageMapped4K { phys: page })
    }
}

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!("mov rax, cr3",
            out("rax") cr3)
    }
    cr3
}

pub const KERNEL_CS: u16 = 1 << 3;
pub const KERNEL_DS: u16 = 2 << 3;
pub const TSS64_SEL: u16 = 6 << 3;

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
	"lea rax, [rip + 2f]", // Target address (label 1 below)
	"push cx", // Construct a far pointer on the stack
	"push rax",
	"ljmp [rsp]",
        "2:",
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
    // Should be aligned on 16-byte boundaries to pass the
    // alignment checks done by FXSAVE / FXRSTOR
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
macro_rules! interrupt_entrypoint_with_ecode {
    ($index:literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
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
interrupt_entrypoint_with_ecode!(8);
interrupt_entrypoint_with_ecode!(13);
interrupt_entrypoint_with_ecode!(14);
interrupt_entrypoint!(32);

extern "sysv64" {
    fn interrupt_entrypoint3();
    fn interrupt_entrypoint6();
    fn interrupt_entrypoint8();
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

pub fn read_cr2() -> u64 {
    let mut cr2: u64;
    unsafe {
        asm!("mov rax, cr2",
            out("rax") cr2)
    }
    cr2
}

#[no_mangle]
extern "sysv64" fn inthandler(info: &InterruptInfo, index: usize) {
    /*
    if index == 32 {
        let bsp_local_apic = BootInfo::take().bsp_local_apic();
        bsp_local_apic.notify_end_of_interrupt();
        return;
    }
    */
    error!("Interrupt Info: {:?}", info);
    error!("Exception {index:#04X}: ");
    match index {
        3 => {
            error!("Breakpoint");
            return;
        }
        6 => {
            error!("Invalid Opcode");
        }
        8 => {
            error!("Double Fault");
        }
        13 => {
            error!("General Protection Fault");
            let rip = info.ctx.rip;
            error!("Bytes @ RIP({rip:#018X}):");
            let rip = rip as *const u8;
            let bytes = unsafe { core::slice::from_raw_parts(rip, 16) };
            error!("  = {bytes:02X?}");
        }
        14 => {
            error!("Page Fault");
            error!("CR2={:#018X}", read_cr2());
            error!(
                "Caused by: A {} mode {} on a {} page, page structures are {}",
                if info.error_code & 0b0000_0100 != 0 {
                    "user"
                } else {
                    "supervisor"
                },
                if info.error_code & 0b0001_0000 != 0 {
                    "instruction fetch"
                } else if info.error_code & 0b0010 != 0 {
                    "data write"
                } else {
                    "data read"
                },
                if info.error_code & 0b0001 != 0 {
                    "present"
                } else {
                    "non-present"
                },
                if info.error_code & 0b1000 != 0 {
                    "invalid"
                } else {
                    "valid"
                },
            );
        }
        _ => {
            error!("Not handled");
        }
    }
    panic!("fatal exception");
}

#[no_mangle]
extern "sysv64" fn int_handler_unimplemented() {
    panic!("unexpected interrupt!");
}

// PDDRTTTT (TTTT: type, R: reserved, D: DPL, P: present)
pub const BIT_FLAGS_INTGATE: u8 = 0b0000_1110u8;
pub const BIT_FLAGS_PRESENT: u8 = 0b1000_0000u8;
pub const BIT_FLAGS_DPL0: u8 = 0 << 5;
pub const BIT_FLAGS_DPL3: u8 = 3 << 5;

#[repr(u8)]
#[derive(Copy, Clone)]
enum IdtAttr {
    // Without _NotPresent value, MaybeUninit::zeroed() on
    // this struct will be undefined behavior.
    _NotPresent = 0,
    IntGateDPL0 = BIT_FLAGS_INTGATE | BIT_FLAGS_PRESENT | BIT_FLAGS_DPL0,
    IntGateDPL3 = BIT_FLAGS_INTGATE | BIT_FLAGS_PRESENT | BIT_FLAGS_DPL3,
}

#[repr(C, packed)]
#[allow(dead_code)]
#[derive(Copy, Clone)]
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
#[repr(C, packed)]
#[derive(Debug)]
struct IdtrParameters {
    limit: u16,
    base: *const IdtDescriptor,
}
const _: () = assert!(size_of::<IdtrParameters>() == 10);
const _: () = assert!(offset_of!(IdtrParameters, base) == 2);

pub struct Idt {
    #[allow(dead_code)]
    entries: Pin<Box<[IdtDescriptor; 0x100]>>,
}
impl Idt {
    pub fn new(segment_selector: u16) -> Self {
        let mut entries = [IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            int_handler_unimplemented,
        ); 0x100];
        entries[3] = IdtDescriptor::new(
            segment_selector,
            1,
            // Set DPL=3 to allow user land to make this interrupt (e.g. via int3 op)
            IdtAttr::IntGateDPL3,
            interrupt_entrypoint3,
        );
        entries[6] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint6,
        );
        entries[8] = IdtDescriptor::new(
            segment_selector,
            2,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint8,
        );
        entries[13] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint13,
        );
        entries[14] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint14,
        );
        entries[32] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0,
            interrupt_entrypoint32,
        );
        let limit = size_of_val(&entries) as u16;
        let entries = Box::pin(entries);
        let params = IdtrParameters {
            limit,
            base: entries.as_ptr(),
        };
        info!("Loading IDT: {params:?}");
        // SAFETY: This is safe since it loads a valid IDT that is constructed in the code just above
        unsafe {
            asm!("lidt [rcx]",
                in("rcx") &params);
        }
        Self { entries }
    }
}

// 7.7 TASK MANAGEMENT IN 64-BIT MODE
#[repr(C, packed)]
struct TaskStateSegment64Inner {
    _reserved0: u32,
    _rsp: [u64; 3], // for switch into ring0-2
    _ist: [u64; 8], // ist[1]~ist[7] (ist[0] is reserved)
    _reserved1: [u16; 5],
    _io_map_base_addr: u16,
}
const _: () = assert!(size_of::<TaskStateSegment64Inner>() == 104);

pub struct TaskStateSegment64 {
    inner: Pin<Box<TaskStateSegment64Inner>>,
}
impl TaskStateSegment64 {
    pub fn phys_addr(&self) -> u64 {
        self.inner.as_ref().get_ref() as *const TaskStateSegment64Inner as u64
    }
    unsafe fn alloc_interrupt_stack() -> u64 {
        const HANDLER_STACK_SIZE: usize = 64 * 1024;
        let stack = Box::new([0u8; HANDLER_STACK_SIZE]);
        let rsp = unsafe { stack.as_ptr().add(HANDLER_STACK_SIZE) as u64 };
        core::mem::forget(stack);
        // now, no one except us own the region since it is forgotten by the allocator ;)
        rsp
    }
    pub fn new() -> Self {
        let rsp0 = unsafe { Self::alloc_interrupt_stack() };
        let mut ist = [0u64; 8];
        for ist in ist[1..=7].iter_mut() {
            *ist = unsafe { Self::alloc_interrupt_stack() };
        }
        let tss64 = TaskStateSegment64Inner {
            _reserved0: 0,
            _rsp: [rsp0, 0, 0],
            _ist: ist,
            _reserved1: [0; 5],
            _io_map_base_addr: 0,
        };
        let this = Self {
            inner: Box::pin(tss64),
        };
        info!("TSS64 created @ {:#X}", this.phys_addr(),);
        this
    }
}
impl Drop for TaskStateSegment64 {
    fn drop(&mut self) {
        panic!("TSS64 being dropped!");
    }
}

pub fn init_exceptions() -> (GdtWrapper, Idt) {
    let gdt = GdtWrapper::default();
    gdt.load();
    unsafe {
        write_cs(KERNEL_CS);
        write_ss(KERNEL_DS);
        write_es(KERNEL_DS);
        write_ds(KERNEL_DS);
        write_fs(KERNEL_DS);
        write_gs(KERNEL_DS);
    }
    let idt = Idt::new(KERNEL_CS);
    (gdt, idt)
}

pub const BIT_TYPE_DATA: u64 = 0b10u64 << 43;
pub const BIT_TYPE_CODE: u64 = 0b11u64 << 43;

pub const BIT_PRESENT: u64 = 1u64 << 47;
pub const BIT_CS_LONG_MODE: u64 = 1u64 << 53;
pub const BIT_CS_READABLE: u64 = 1u64 << 53;
pub const BIT_DS_WRITABLE: u64 = 1u64 << 41;
pub const BIT_DPL0: u64 = 0u64 << 45;
pub const BIT_DPL3: u64 = 3u64 << 45;

#[repr(u64)]
enum GdtAttr {
    KernelCode = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE,
    KernelData = BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE,
    User64Code = BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE | BIT_DPL3,
    UserData = BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE | BIT_DPL3,
}

#[allow(dead_code)]
#[repr(C, packed)]
struct GdtrParameters {
    limit: u16,
    base: *const Gdt,
}

#[allow(dead_code)]
#[repr(C, packed)]
pub struct Gdt {
    null_segment: GdtSegmentDescriptor,
    kernel_code_segment: GdtSegmentDescriptor,
    kernel_data_segment: GdtSegmentDescriptor,
    user_code_segment_32: GdtSegmentDescriptor,
    user_data_segment: GdtSegmentDescriptor,
    user_code_segment_64: GdtSegmentDescriptor,
    task_state_segment: TaskStateSegment64Descriptor,
}
const _: () = assert!(size_of::<Gdt>() == 64);

#[allow(dead_code)]
pub struct GdtWrapper {
    inner: Pin<Box<Gdt>>,
    tss64: TaskStateSegment64,
}

impl GdtWrapper {
    pub fn load(&self) {
        let params = GdtrParameters {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self.inner.as_ref().get_ref() as *const Gdt,
        };
        info!("Loading GDT @ {:#018X}", params.base as u64);
        // SAFETY: This is safe since it is loading a valid GDT just constructed in the above
        unsafe {
            asm!("lgdt [rcx]",
                in("rcx") &params);
        }
        info!("Loading TSS ( selector = {:#X} )", TSS64_SEL);
        unsafe {
            asm!("ltr cx",
                in("cx") TSS64_SEL);
        }
    }
}
impl Default for GdtWrapper {
    fn default() -> Self {
        let tss64 = TaskStateSegment64::new();
        let gdt = Gdt {
            null_segment: GdtSegmentDescriptor::null(),
            kernel_code_segment: GdtSegmentDescriptor::new(GdtAttr::KernelCode),
            kernel_data_segment: GdtSegmentDescriptor::new(GdtAttr::KernelData),
            user_code_segment_32: GdtSegmentDescriptor::null(),
            user_data_segment: GdtSegmentDescriptor::new(GdtAttr::UserData),
            user_code_segment_64: GdtSegmentDescriptor::new(GdtAttr::User64Code),
            task_state_segment: TaskStateSegment64Descriptor::new(tss64.phys_addr()),
        };
        let gdt = Box::pin(gdt);
        GdtWrapper { inner: gdt, tss64 }
    }
}

pub struct GdtSegmentDescriptor {
    value: u64,
}
impl GdtSegmentDescriptor {
    const fn null() -> Self {
        Self { value: 0 }
    }
    const fn new(attr: GdtAttr) -> Self {
        Self { value: attr as u64 }
    }
}
impl fmt::Display for GdtSegmentDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#18X}", self.value)
    }
}

#[repr(C, packed)]
#[allow(dead_code)]
struct TaskStateSegment64Descriptor {
    limit_low: u16,
    base_low: u16,
    base_mid_low: u8,
    attr: u16,
    base_mid_high: u8,
    base_high: u32,
    reserved: u32,
}
impl TaskStateSegment64Descriptor {
    const fn new(base_addr: u64) -> Self {
        Self {
            limit_low: size_of::<TaskStateSegment64Inner>() as u16,
            base_low: (base_addr & 0xffff) as u16,
            base_mid_low: ((base_addr >> 16) & 0xff) as u8,
            attr: 0b1000_0000_1000_1001,
            base_mid_high: ((base_addr >> 24) & 0xff) as u8,
            base_high: ((base_addr >> 32) & 0xffffffff) as u32,
            reserved: 0,
        }
    }
}
const _: () = assert!(size_of::<TaskStateSegment64Descriptor>() == 16);

pub fn trigger_debug_interrupt() {
    unsafe { asm!("int3") }
}

/// # Safety
/// Writing to CR3 can causes any exceptions so it is
/// programmer's responsibility to setup correct page tables.
#[no_mangle]
pub unsafe fn write_cr3(table: *const PML4) {
    asm!("mov cr3, rax",
            in("rax") table)
}

pub fn flush_tlb() {
    unsafe {
        write_cr3(read_cr3());
    }
}

/// # Safety
/// This will create a mutable reference to the page table structure
/// So is is programmer's responsibility to ensure that at most one
/// instance of the reference exist at every moment.
pub unsafe fn take_current_page_table() -> ManuallyDrop<Box<PML4>> {
    ManuallyDrop::new(Box::from_raw(read_cr3()))
}
/// # Safety
/// This function sets the CR3 value so that anything bad can happen.
pub unsafe fn put_current_page_table(mut table: ManuallyDrop<Box<PML4>>) {
    // Set CR3 to reflect the updates and drop TLB caches.
    write_cr3(Box::into_raw(ManuallyDrop::take(&mut table)))
}
/// # Safety
/// This function modifies the page table as callback does, so
/// anything bad can happen if there are some mistakes.
pub unsafe fn with_current_page_table<F>(callback: F)
where
    F: FnOnce(&mut PML4),
{
    let mut table = take_current_page_table();
    callback(&mut table);
    put_current_page_table(table)
}

pub fn disable_cache<T: Sized>(io_box: &IoBox<T>) {
    let region = io_box.as_ref();
    let vstart = region as *const T as u64;
    let vend = vstart + size_of_val(region) as u64;
    unsafe {
        with_current_page_table(|pt| {
            pt.create_mapping(vstart, vend, vstart, PageAttr::ReadWriteIo)
                .expect("Failed to create mapping")
        })
    }
}
