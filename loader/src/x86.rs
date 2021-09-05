use core::fmt;

pub fn write_io_port(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al",
            in("al") data,
            in("dx") port)
    }
}

pub fn read_io_port(port: u16) -> u8 {
    let mut data: u8;
    unsafe {
        asm!("in al, dx",
            out("al") data,
            in("dx") port)
    }
    data
}

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!("mov rax, cr3",
            out("rax") cr3)
    }
    cr3
}

pub trait PageTableEntry {
    fn read_value(&self) -> u64;
    fn read_name() -> &'static str;
    fn format_additional_attrs(&self, _f: &mut fmt::Formatter) -> fmt::Result {
        Result::Ok(())
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
            "{:9} {{ {:#018X} {}{}{} ",
            Self::read_name(),
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" }
        )?;
        self.format_additional_attrs(f)?;
        write!(f, " }}")
    }
}

#[derive(Debug)]
pub struct PTEntry {
    value: u64,
}
impl PageTableEntry for PTEntry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PTEntry"
    }
}
impl fmt::Display for PTEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
pub struct PDEntry {
    value: u64,
}
impl PageTableEntry for PDEntry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PDEntry"
    }
    fn format_additional_attrs(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.read_value() & 0x80 != 0 {
            write!(f, "2MBPage")?;
        }
        Result::Ok(())
    }
}
impl fmt::Display for PDEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
pub struct PDPTEntry {
    value: u64,
}
impl PageTableEntry for PDPTEntry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PDPTEntry"
    }
}
impl fmt::Display for PDPTEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

#[derive(Debug)]
pub struct PML4Entry {
    value: u64,
}
impl PageTableEntry for PML4Entry {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn read_name() -> &'static str {
        "PML4Entry"
    }
}
impl fmt::Display for PML4Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}

pub trait PageTable<T: 'static + PageTableEntry + core::fmt::Debug + core::fmt::Display> {
    fn read_entry(&self, index: usize) -> &T;
    fn read_name() -> &'static str;
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:5} @ {:#p} {{", Self::read_name(), self)?;
        for i in 0..512 {
            let e = self.read_entry(i);
            if !e.is_present() {
                continue;
            }
            writeln!(f, "  entry[{:3}] = {}", i, e)?;
        }
        writeln!(f, "}}")?;
        Result::Ok(())
    }
}
#[derive(Debug)]
pub struct PT {
    pub entry: [PTEntry; 512],
}
impl PageTable<PTEntry> for PT {
    fn read_entry(&self, index: usize) -> &PTEntry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PT"
    }
}
impl fmt::Display for PT {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
pub fn get_pt(e: &PDEntry) -> &'static mut PT {
    unsafe { &mut *(((e.read_value() & !0xFFF) as usize) as *mut PT) }
}

#[derive(Debug)]
pub struct PD {
    pub entry: [PDEntry; 512],
}
impl PageTable<PDEntry> for PD {
    fn read_entry(&self, index: usize) -> &PDEntry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PD"
    }
}
impl fmt::Display for PD {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
pub fn get_pd(e: &PDPTEntry) -> &'static mut PD {
    unsafe { &mut *(((e.read_value() & !0xFFF) as usize) as *mut PD) }
}

#[derive(Debug)]
pub struct PDPT {
    pub entry: [PDPTEntry; 512],
}
impl PageTable<PDPTEntry> for PDPT {
    fn read_entry(&self, index: usize) -> &PDPTEntry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PDPT"
    }
}
impl fmt::Display for PDPT {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
pub fn get_pdpt(e: &PML4Entry) -> &'static mut PDPT {
    unsafe { &mut *(((e.read_value() & !0xFFF) as usize) as *mut PDPT) }
}

#[derive(Debug)]
pub struct PML4 {
    pub entry: [PML4Entry; 512],
}
impl PageTable<PML4Entry> for PML4 {
    fn read_entry(&self, index: usize) -> &PML4Entry {
        &self.entry[index]
    }
    fn read_name() -> &'static str {
        "PML4"
    }
}
impl fmt::Display for PML4 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.format(f)
    }
}
