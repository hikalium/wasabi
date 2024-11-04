use core::cmp::min;

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
