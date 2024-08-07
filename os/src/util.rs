extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use core::cmp::min;
use core::convert::From;
use core::convert::TryInto;
use core::mem::size_of;
use core::pin::Pin;
use core::slice;

pub const PAGE_OFFSET_BITS: usize = 12;
pub const PAGE_SIZE: usize = 1 << PAGE_OFFSET_BITS;

/// # Safety
/// Implementing this trait is safe only when the target type can be constructed from any byte
/// sequences that has the same size. If not, modification made via the byte slice produced by
/// as_mut_slice can be an undefined behavior since the bytes can not be interpreted as the
/// original type.
pub unsafe trait IntoPinnedMutableSlice: Sized + Copy + Clone {
    fn as_mut_slice(self: Pin<&mut Self>) -> Pin<&mut [u8]> {
        Pin::new(unsafe {
            slice::from_raw_parts_mut(
                self.get_unchecked_mut() as *mut Self as *mut u8,
                size_of::<Self>(),
            )
        })
    }
    fn as_mut_slice_sized(self: Pin<&mut Self>, size: usize) -> Result<Pin<&mut [u8]>> {
        if size > size_of::<Self>() {
            Err(Error::Failed(
                "Cannot take mut slice longer than the object",
            ))
        } else {
            Ok(Pin::new(unsafe {
                slice::from_raw_parts_mut(self.get_unchecked_mut() as *mut Self as *mut u8, size)
            }))
        }
    }
}

pub fn extract_bits<T>(value: T, shift: usize, width: usize) -> T
where
    T: TryFrom<u64> + From<u8>,
    u64: TryInto<T> + From<T>,
{
    let mask = (1u64 << min(63, width)) - 1;
    let value = u64::from(value);
    let value = value.checked_shr(shift as u32).unwrap_or(0) & mask;
    TryInto::try_into(value).unwrap_or_else(|_| T::from(0u8))
}

#[test_case]
fn extract_bits_tests() {
    assert_eq!(extract_bits(30u32 << 24, 24, 8), 30u32);
    assert_eq!(extract_bits(0x123u64, 0, 12), 0x123u64);
    assert_eq!(extract_bits(0x123u64, 4, 12), 0x12u64);
    assert_eq!(extract_bits(0x123u64, 4, 8), 0x12u64);
    assert_eq!(extract_bits(0x123u64, 4, 4), 0x2u64);
    assert_eq!(extract_bits(0x123u64, 4, 0), 0x0u64);
    assert_eq!(extract_bits(0x1234_5678_1234_5678u64, 60, 4), 0x1u64);
    assert_eq!(extract_bits(0x1234_5678_1234_5678u64, 64, 0), 0x0u64);
    assert_eq!(
        extract_bits(0x1234_5678_1234_5678u64, 0, 64),
        0x1234_5678_1234_5678u64
    );
    assert_eq!(
        extract_bits(0x1234_5678_1234_5678u64, 0, 65),
        0x1234_5678_1234_5678u64
    );
}

pub fn size_in_pages_from_bytes(size_in_bytes: usize) -> usize {
    (size_in_bytes + PAGE_SIZE - 1) >> PAGE_OFFSET_BITS
}

pub fn round_up_to_nearest_pow2(v: usize) -> Result<usize> {
    1usize
        .checked_shl(usize::BITS - v.wrapping_sub(1).leading_zeros())
        .ok_or(Error::CalcOutOfRange)
}
#[test_case]
fn round_up_to_nearest_pow2_tests() {
    assert_eq!(round_up_to_nearest_pow2(0), Err(Error::CalcOutOfRange));
    assert_eq!(round_up_to_nearest_pow2(1), Ok(1));
    assert_eq!(round_up_to_nearest_pow2(2), Ok(2));
    assert_eq!(round_up_to_nearest_pow2(3), Ok(4));
    assert_eq!(round_up_to_nearest_pow2(4), Ok(4));
    assert_eq!(round_up_to_nearest_pow2(5), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(6), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(7), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(8), Ok(8));
    assert_eq!(round_up_to_nearest_pow2(9), Ok(16));
    assert_eq!(round_up_to_nearest_pow2(9), Ok(16));
}

pub fn read_le_u16(data: &[u8], ofs: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        data[ofs..(ofs + 2)]
            .try_into()
            .map_err(|_| Error::Failed("Failed to convert slice into array"))?,
    ))
}
pub fn read_le_u32(data: &[u8], ofs: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        data[ofs..(ofs + 4)]
            .try_into()
            .map_err(|_| Error::Failed("Failed to convert slice into array"))?,
    ))
}
pub fn read_le_u64(data: &[u8], ofs: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        data[ofs..(ofs + 8)]
            .try_into()
            .map_err(|_| Error::Failed("Failed to convert slice into array"))?,
    ))
}
pub fn write_le_u64(data: &mut [u8], ofs: usize, value: u64) -> Result<()> {
    unsafe {
        (data.as_mut_ptr().add(ofs) as *mut u64).write(value);
    }
    Ok(())
}
