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
        // https://wiki.osdev.org/ELF#Header
        if &data[0..4] != b"\x7fELF".as_slice() {
            return Err(WasabiError::Failed("No ELF signature found"));
        }
        if data[4] != 2 {
            return Err(WasabiError::Failed("Not a 64-bit ELF"));
        }
        if data[5] != 1 {
            return Err(WasabiError::Failed("Not a litte endian ELF"));
        }
        if data[7] != 0 {
            return Err(WasabiError::Failed("ABI is not SystemV"));
        }
        if u16::from_le_bytes(
            data[16..=17]
                .try_into()
                .map_err(|_| WasabiError::Failed("Failed to convert slice into array"))?,
        ) != 2
        {
            return Err(WasabiError::Failed("Not an executable ELF"));
        }
        println!("This ELF seems to be executable!");
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
