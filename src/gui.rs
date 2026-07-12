use crate::graphics::Bitmap;
use crate::mutex::Mutex;
use crate::uefi::VramBufferInfo;

pub static GLOBAL_VRAM: Mutex<VramBufferInfo> =
    Mutex::new(VramBufferInfo::null());

pub fn set_global_vram(vram: VramBufferInfo) {
    *GLOBAL_VRAM.lock() = vram;
}
pub fn global_vram_resolutions() -> (i64, i64) {
    let vram = GLOBAL_VRAM.lock();
    (vram.width(), vram.height())
}
