use crate::efi::*;

pub const MEMORY_MAP_BUFFER_SIZE: usize = 0x8000;

pub struct MemoryMapHolder {
    pub memory_map_buffer: [u8; MEMORY_MAP_BUFFER_SIZE],
    pub memory_map_size: usize,
    pub map_key: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
}
pub struct MemoryMapIterator<'a> {
    map: &'a MemoryMapHolder,
    ofs: usize,
}
impl<'a> Iterator for MemoryMapIterator<'a> {
    type Item = &'a EfiMemoryDescriptor;
    fn next(&mut self) -> Option<&'a EfiMemoryDescriptor> {
        if self.ofs >= self.map.memory_map_size {
            None
        } else {
            let e: &EfiMemoryDescriptor = unsafe {
                &*(self.map.memory_map_buffer.as_ptr().add(self.ofs as usize)
                    as *const EfiMemoryDescriptor)
            };
            self.ofs += self.map.descriptor_size;
            Some(e)
        }
    }
}

impl MemoryMapHolder {
    pub const fn new() -> MemoryMapHolder {
        MemoryMapHolder {
            memory_map_buffer: [0; MEMORY_MAP_BUFFER_SIZE],
            memory_map_size: MEMORY_MAP_BUFFER_SIZE as usize,
            map_key: 0,
            descriptor_size: 0,
            descriptor_version: 0,
        }
    }
    pub fn iter(&self) -> MemoryMapIterator {
        MemoryMapIterator { map: self, ofs: 0 }
    }
}

pub fn get_memory_map(efi_system_table: &EfiSystemTable, map: &mut MemoryMapHolder) -> EfiStatus {
    (efi_system_table.boot_services().get_memory_map)(
        &mut map.memory_map_size,
        map.memory_map_buffer.as_mut_ptr(),
        &mut map.map_key,
        &mut map.descriptor_size,
        &mut map.descriptor_version,
    )
}
