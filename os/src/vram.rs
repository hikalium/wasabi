use crate::efi::{locate_graphic_protocol, EFISystemTable};
use crate::error::*;
use graphics::BitmapImageBuffer;

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
    fn buf(&self) -> *mut u8 {
        self.buf
    }
    unsafe fn pixel_at(&self, x: i64, y: i64) -> *mut u8 {
        self.buf()
            .add(((y * self.pixels_per_line() + x) * self.bytes_per_pixel()) as usize)
    }
    fn flush(&self) {
        // Do nothing
    }
    fn is_in_x_range(&self, px: i64) -> bool {
        0 <= px && px < self.width as i64
    }
    fn is_in_y_range(&self, py: i64) -> bool {
        0 <= py && py < self.height as i64
    }
}

pub fn init_vram(efi_system_table: &EFISystemTable) -> Result<VRAMBufferInfo, WasabiError> {
    let gp = locate_graphic_protocol(efi_system_table)?;
    Ok(VRAMBufferInfo {
        buf: gp.mode.frame_buffer_base as *mut u8,
        width: gp.mode.info.horizontal_resolution as usize,
        height: gp.mode.info.vertical_resolution as usize,
        pixels_per_line: gp.mode.info.pixels_per_scan_line as usize,
    })
}
