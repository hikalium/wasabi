extern crate alloc;

use crate::result::Result;

// [ncm_1_1] 3.2.1 NCM Transfer Header (16-bit), NTH16
//
//   off  size  field
//     0     4  dwSignature       "NCMH"
//     4     2  wHeaderLength     = 0x000C
//     6     2  wSequence
//     8     2  wBlockLength      total NTB length in bytes
//    10     2  wNdpIndex         byte offset of the first NDP16
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Nth16 {
    pub sequence: u16,
    pub block_length: u16,
    pub ndp_index: u16,
}

pub fn parse_nth16(buf: &[u8]) -> Result<Nth16> {
    if buf.len() < 12 {
        return Err("NTH16: buffer too short");
    }
    if &buf[0..4] != b"NCMH" {
        return Err("NTH16: bad signature");
    }
    let header_length = u16::from_le_bytes([buf[4], buf[5]]);
    if header_length != 0x000C {
        return Err("NTH16: unexpected wHeaderLength");
    }
    Ok(Nth16 {
        sequence: u16::from_le_bytes([buf[6], buf[7]]),
        block_length: u16::from_le_bytes([buf[8], buf[9]]),
        ndp_index: u16::from_le_bytes([buf[10], buf[11]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn parse_nth16_known_bytes() {
        // A minimal well-formed NTH16: seq=0x0042, block_length=64,
        // ndp_index=12.
        let buf: [u8; 12] = [
            b'N', b'C', b'M', b'H', // dwSignature
            0x0C, 0x00, // wHeaderLength = 12
            0x42, 0x00, // wSequence    = 0x0042
            0x40, 0x00, // wBlockLength = 64
            0x0C, 0x00, // wNdpIndex    = 12
        ];
        let nth = parse_nth16(&buf).unwrap();
        assert_eq!(nth.sequence, 0x0042);
        assert_eq!(nth.block_length, 64);
        assert_eq!(nth.ndp_index, 12);
    }

    #[test_case]
    fn parse_nth16_rejects_bad_signature() {
        let mut buf = [0u8; 12];
        buf[..4].copy_from_slice(b"XXXX");
        buf[4] = 0x0C;
        assert!(parse_nth16(&buf).is_err());
    }

    #[test_case]
    fn parse_nth16_rejects_short_buffer() {
        let buf = [0u8; 4];
        assert!(parse_nth16(&buf).is_err());
    }
}
