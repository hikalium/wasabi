use crate::efi::locate_graphic_protocol;
use crate::efi::EfiSystemTable;
use crate::error::Result;
use crate::graphics::BitmapImageBuffer;
use core::pin::Pin;

#[derive(Clone, Copy)]
pub struct VRAMBufferInfo {
    buf: *mut u8,
    width: usize,
    height: usize,
    pixels_per_line: usize,
}

impl BitmapImageBuffer for VRAMBufferInfo {
    fn bytes_per_pixel(&self) -> i64 {
        4
    }
    fn pixels_per_line(&self) -> i64 {
        self.pixels_per_line as i64
    }
    fn width(&self) -> i64 {
        self.width as i64
    }
    fn height(&self) -> i64 {
        self.height as i64
    }
    fn buf(&self) -> *const u8 {
        self.buf
    }
    fn buf_mut(&mut self) -> *mut u8 {
        self.buf
    }
}

pub fn init_vram(efi_system_table: Pin<&EfiSystemTable>) -> Result<VRAMBufferInfo> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VRAMBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as usize,
        height: gp.mode.info.vertical_resolution as usize,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as usize,
    })
}
