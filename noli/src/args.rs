extern crate alloc;

use crate::error::Error;
use crate::error::Result;
use crate::prelude::*;
use alloc::vec::Vec;

/// Serialize the arguments into bytes.
/// Format:
/// - total_size: u64
/// - num_args: u64
/// - ofs_and_len: (ofs: u64, len: u64) * num_args
/// - data
/// Each arg data is null-terminated, but "len" of an arg
/// does not count the null terminator.
/// For example, args ["zero", "one", "two"] will be serialized as:
/// - total_size = 77
/// - num_args = 3
/// - ofs_and_len:
///   index 0: (ofs = 64, len = 4)
///   index 1: (ofs = 69, len = 3)
///   index 2: (ofs = 73, len = 3)
/// - data = "zero\0one\0two\0"
pub fn serialize_args(args: &[&str]) -> Vec<u8> {
    let args: Vec<&[u8]> = args.iter().map(|e| e.as_bytes()).collect();
    let data_base_ofs: u64 = (args.len() as u64 + 1) * 16;
    let mut ofs_and_len: Vec<(u64, u64)> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    for e in &args {
        ofs_and_len.push((data_base_ofs + data.len() as u64, e.len() as u64));
        data.extend_from_slice(e);
        data.push(0);
    }
    let total_size = data_base_ofs + data.len() as u64;
    let num_args = args.len() as u64;
    let mut serialized: Vec<u8> = Vec::new();
    serialized.extend_from_slice(&total_size.to_le_bytes());
    serialized.extend_from_slice(&num_args.to_le_bytes());
    for e in ofs_and_len {
        serialized.extend_from_slice(&e.0.to_le_bytes());
        serialized.extend_from_slice(&e.1.to_le_bytes());
    }
    serialized.extend_from_slice(&data);
    serialized
}

pub fn deserialize_args(data: &[u8]) -> Result<Vec<&str>> {
    let mut tmp = [0u8; 8];

    if data.len() < 16 {
        return Err(Error::Failed("Invalid args data"));
    }

    tmp.copy_from_slice(&data[8..16]);
    let num_args = u64::from_le_bytes(tmp) as usize;
    if (num_args + 1) * 16 > data.len() {
        return Err(Error::Failed("Invalid args data"));
    }

    let mut args = Vec::new();
    for i in 0..num_args {
        tmp.copy_from_slice(&data[(16 + i * 16)..(16 + i * 16 + 8)]);
        let ofs = u64::from_le_bytes(tmp) as usize;
        tmp.copy_from_slice(&data[(16 + i * 16 + 8)..(16 + i * 16 + 16)]);
        let len = u64::from_le_bytes(tmp) as usize;
        let s = core::str::from_utf8(&data[ofs..(ofs + len)]);
        if let Ok(s) = s {
            args.push(s);
        } else {
            return Err(Error::Failed("Invalid args data"));
        }
    }
    Ok(args)
}

pub fn from_env() -> Vec<&'static str> {
    let args = Api::get_args_region();
    args.and_then(|args| deserialize_args(args).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // For example, args ["zero", "one", "two"] will be
    // serialized as follows:
    // - total_size = 77
    // - num_args = 3
    // - ofs_and_len:
    //   [0]: (ofs = 64, len = 4)
    //   [1]: (ofs = 69, len = 3)
    //   [2]: (ofs = 73, len = 3)
    // - data = "zero\0one\0two\0"
    use super::alloc::vec;
    use super::alloc::vec::Vec;
    use super::deserialize_args;
    use super::serialize_args;

    fn expected_bytes() -> Vec<u8> {
        let mut expected = Vec::new();
        expected.extend_from_slice(&77u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&64u64.to_le_bytes());
        expected.extend_from_slice(&4u64.to_le_bytes());
        expected.extend_from_slice(&69u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&73u64.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice("zero\0one\0two\0".as_bytes());
        expected
    }
    fn expected_args() -> Vec<&'static str> {
        vec!["zero", "one", "two"]
    }

    #[test]
    fn test_serialize_args() {
        let actual = serialize_args(&expected_args());
        assert_eq!(actual, expected_bytes());
    }

    #[test]
    fn test_deserialize_args() {
        let input = expected_bytes();
        let actual = deserialize_args(&input).unwrap();
        assert_eq!(actual, expected_args());
    }
}
