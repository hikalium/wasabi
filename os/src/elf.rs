use crate::boot_info::File;
use crate::error::Result;
use crate::error::WasabiError;
use crate::print::hexdump;
use crate::println;
use core::fmt;

pub struct Elf<'a> {
    file: &'a File,
}
impl<'a> Elf<'a> {
    pub fn new(file: &'a File) -> Self {
        Self { file }
    }
    pub fn parse(&self) -> Result<()> {
        hexdump(self.file.data());
        let data = self.file.data();
        if &data[0..4] != b"\x7fELF".as_slice() {
            return Err(WasabiError::Failed("No ELF signature found"));
        }
        println!("ELF signature found.");
        Ok(())
    }
}
impl<'a> fmt::Debug for Elf<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Elf {{ name: {}, data: @{:#p} }}",
            &self.file.name(),
            self.file.data().as_ptr()
        )
    }
}
