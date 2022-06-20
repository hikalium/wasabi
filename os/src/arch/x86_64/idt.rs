use crate::println;
use core::arch::asm;
use core::arch::global_asm;
use core::cell::RefCell;
use core::mem::MaybeUninit;

global_asm!(
    r#"
.global asm_int06
asm_int06:
    push 0 // No error code
    jmp int06
"#
);

extern "C" {
    fn asm_int06();
}

#[no_mangle]
extern "C" fn int06() {
    panic!("Exception 0x06: Undefined Opcode");
}

#[no_mangle]
extern "C" fn int_handler_unimplemented() {
    println!("hey!!!");
    panic!("unexpected interrupt!");
}

mod attr_bits {
    // PDDRTTTT (TTTT: type, R: reserved, D: DPL, P: present)
    pub const BIT_FLAGS_INTGATE: u8 = 0b0000_1110u8;
    pub const BIT_FLAGS_PRESENT: u8 = 0b1000_0000u8;
}
use attr_bits::*;

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
const _: () = assert!(core::mem::size_of::<IdtDescriptor>() == 16);
impl IdtDescriptor {
    fn new(segment_selector: u16, ist_index: u8, attr: IdtAttr, f: unsafe extern "C" fn()) -> Self {
        let handler_addr = f as *const unsafe extern "C" fn() as usize;
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
        entries[0x06] = IdtDescriptor::new(segment_selector, 0, IdtAttr::IntGateDPL0, asm_int06);
    }
    /// # Safety
    /// It is programmer's responsible to call this method
    /// with a valid, correct IDT.
    pub unsafe fn load(&'static self) {
        let entries = &*self.entries.borrow();
        let params = IdtrParameters {
            limit: (core::mem::size_of_val(entries) - 1) as u16,
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

/*

  IDTGateDescriptor descriptors_[256];

enum class IDTType {
  kInterruptGate = 0xE,
  kTrapGate = 0xF,
};

packed_struct IDTGateDescriptor {
  uint16_t offset_low;
  uint16_t segment_descriptor;
  unsigned interrupt_stack_table : 3;
  unsigned reserved0 : 5;
  unsigned type : 4;
  unsigned reserved1 : 1;
  unsigned descriptor_privilege_level : 2;
  unsigned present : 1;
  unsigned offset_mid : 16;
  uint32_t offset_high;
  uint32_t reserved2;
};

packed_struct IDTR {
  uint16_t limit;
  IDTGateDescriptor* base;
};

void IDT::SetEntry(int index,
                   uint8_t segm_desc,
                   uint8_t ist,
                   IDTType type,
                   uint8_t dpl,
                   __attribute__((ms_abi)) void (*handler)()) {
  IDTGateDescriptor* desc = &descriptors_[index];
  desc->segment_descriptor = segm_desc;
  desc->interrupt_stack_table = ist;
  desc->type = static_cast<int>(type);
  desc->descriptor_privilege_level = dpl;
  desc->present = 1;
  desc->offset_low = (uint64_t)handler & 0xffff;
  desc->offset_mid = ((uint64_t)handler >> 16) & 0xffff;
  desc->offset_high = ((uint64_t)handler >> 32) & 0xffffffff;
  desc->reserved0 = 0;
  desc->reserved1 = 0;
  desc->reserved2 = 0;
}

void IDT::Init() {
  assert(!idt_);
  static uint8_t idt_buf[sizeof(IDT)];
  idt_ = reinterpret_cast<IDT*>(idt_buf);
  idt_->InitInternal();
}

void IDT::InitInternal() {
  uint16_t cs = ReadCSSelector();

  IDTR idtr;
  idtr.limit = sizeof(descriptors_) - 1;
  idtr.base = descriptors_;

  for (int i = 0; i < 0x100; i++) {
    SetEntry(i, cs, 1, IDTType::kInterruptGate, 0, AsmIntHandlerNotImplemented);
    handler_list_[i] = nullptr;
  }

  SetEntry(0x00, cs, 0, IDTType::kInterruptGate, 0,
           AsmIntHandler00_DivideError);
  SetEntry(0x03, cs, 0, IDTType::kInterruptGate, 0, AsmIntHandler03);
  SetEntry(0x06, cs, 0, IDTType::kInterruptGate, 0, AsmIntHandler06);
  SetEntry(0x07, cs, 0, IDTType::kInterruptGate, 0,
           AsmIntHandler07_DeviceNotAvailable);
  SetEntry(0x08, cs, 1, IDTType::kInterruptGate, 0, AsmIntHandler08);
  SetEntry(0x0d, cs, 0, IDTType::kInterruptGate, 0, AsmIntHandler0D);
  SetEntry(0x0e, cs, 1, IDTType::kInterruptGate, 0, AsmIntHandler0E);
  SetEntry(0x10, cs, 0, IDTType::kInterruptGate, 0,
           AsmIntHandler10_x87FPUError);
  SetEntry(0x13, cs, 0, IDTType::kInterruptGate, 0,
           AsmIntHandler13_SIMDFPException);
  SetEntry(0x20, cs, 0, IDTType::kInterruptGate, 0, AsmIntHandler20);
  SetEntry(0x21, cs, 0, IDTType::kInterruptGate, 0, AsmIntHandler21);
  SetEntry(0x22, cs, 0, IDTType::kInterruptGate, 0, AsmIntHandler22);
  WriteIDTR(&idtr);

*/
