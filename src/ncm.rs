extern crate alloc;

use crate::result::Result;
use alloc::vec::Vec;

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

// [ncm_1_1] 3.3.1 NCM Datagram Pointer (16-bit), NDP16 (no CRC)
//
//   off  size  field
//     0     4  dwSignature       "NCM0"
//     4     2  wLength           NDP16 length in bytes
//     6     2  wNextNdpIndex     0 = no further NDP
//     8     2  wDatagramIndex(0)
//    10     2  wDatagramLength(0)
//    12     2  wDatagramIndex(1)  = 0  (terminator)
//    14     2  wDatagramLength(1) = 0  (terminator)
//
// One datagram + terminator => NDP16 length = 16 bytes.
//
// Layout produced by build_ntb16 for a single datagram:
//
//   off  0..12  : NTH16
//   off 12..28  : NDP16
//   off 28..    : datagram bytes
//
// 28 is already a multiple of 4, satisfying the conservative wNdpAlignment = 4.
pub fn build_ntb16(datagram: &[u8], seq: u16) -> Vec<u8> {
    const NTH16_LEN: usize = 12;
    const NDP16_LEN: usize = 16;
    const NDP_INDEX: usize = NTH16_LEN;
    const DATAGRAM_INDEX: usize = NTH16_LEN + NDP16_LEN;

    let block_length = DATAGRAM_INDEX + datagram.len();
    let mut out = alloc::vec![0u8; block_length];

    // NTH16
    out[0..4].copy_from_slice(b"NCMH");
    out[4..6].copy_from_slice(&(NTH16_LEN as u16).to_le_bytes());
    out[6..8].copy_from_slice(&seq.to_le_bytes());
    out[8..10].copy_from_slice(&(block_length as u16).to_le_bytes());
    out[10..12].copy_from_slice(&(NDP_INDEX as u16).to_le_bytes());

    // NDP16 (no-CRC variant: "NCM0")
    out[12..16].copy_from_slice(b"NCM0");
    out[16..18].copy_from_slice(&(NDP16_LEN as u16).to_le_bytes());
    out[18..20].copy_from_slice(&0u16.to_le_bytes()); // wNextNdpIndex
    out[20..22].copy_from_slice(&(DATAGRAM_INDEX as u16).to_le_bytes());
    out[22..24].copy_from_slice(&(datagram.len() as u16).to_le_bytes());
    // out[24..28] = zero terminator (already zero)

    out[DATAGRAM_INDEX..].copy_from_slice(datagram);
    out
}

// [ncm_1_1] 3.3.1: walk an NTB's first NDP16 (no-CRC variant) and yield each
// datagram as a slice into the original buffer. Multi-NDP chaining via
// wNextNdpIndex is not followed; in practice devices put all datagrams in
// the first NDP. Returns an empty iterator on any framing error so callers
// can simply loop without explicit error handling.
pub struct Ntb16DatagramIter<'a> {
    ntb: &'a [u8],
    ndp_offset: usize,
    pair_index: usize,
}

impl<'a> Iterator for Ntb16DatagramIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let entry_off = self.ndp_offset + 8 + self.pair_index * 4;
        if entry_off + 4 > self.ntb.len() {
            return None;
        }
        let idx =
            u16::from_le_bytes([self.ntb[entry_off], self.ntb[entry_off + 1]])
                as usize;
        let len = u16::from_le_bytes([
            self.ntb[entry_off + 2],
            self.ntb[entry_off + 3],
        ]) as usize;
        if idx == 0 && len == 0 {
            return None; // (0,0) end-of-list terminator
        }
        if idx
            .checked_add(len)
            .map_or(true, |end| end > self.ntb.len())
        {
            return None;
        }
        self.pair_index += 1;
        Some(&self.ntb[idx..idx + len])
    }
}

pub fn iter_ntb16_datagrams(ntb: &[u8]) -> Ntb16DatagramIter<'_> {
    if let Ok(nth) = parse_nth16(ntb) {
        let ndp_offset = nth.ndp_index as usize;
        if ntb.len() >= ndp_offset + 8
            && &ntb[ndp_offset..ndp_offset + 4] == b"NCM0"
        {
            return Ntb16DatagramIter {
                ntb,
                ndp_offset,
                pair_index: 0,
            };
        }
    }
    Ntb16DatagramIter {
        ntb,
        ndp_offset: ntb.len(),
        pair_index: 0,
    }
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

    #[test_case]
    fn build_ntb16_layout_for_42_byte_frame() {
        // 42-byte frame (size of an Ethernet+ARP packet) of all 0xAB.
        let frame = [0xABu8; 42];
        let ntb = build_ntb16(&frame, 0x0042);

        // Block length = 12 (NTH) + 16 (NDP) + 42 (frame) = 70.
        assert_eq!(ntb.len(), 70);

        // NTH16
        assert_eq!(&ntb[0..4], b"NCMH");
        assert_eq!(u16::from_le_bytes([ntb[4], ntb[5]]), 0x000C);
        assert_eq!(u16::from_le_bytes([ntb[6], ntb[7]]), 0x0042);
        assert_eq!(u16::from_le_bytes([ntb[8], ntb[9]]), 70);
        assert_eq!(u16::from_le_bytes([ntb[10], ntb[11]]), 12);

        // NDP16
        assert_eq!(&ntb[12..16], b"NCM0");
        assert_eq!(u16::from_le_bytes([ntb[16], ntb[17]]), 16);
        assert_eq!(u16::from_le_bytes([ntb[18], ntb[19]]), 0);
        assert_eq!(u16::from_le_bytes([ntb[20], ntb[21]]), 28);
        assert_eq!(u16::from_le_bytes([ntb[22], ntb[23]]), 42);
        // Terminator
        assert_eq!(u16::from_le_bytes([ntb[24], ntb[25]]), 0);
        assert_eq!(u16::from_le_bytes([ntb[26], ntb[27]]), 0);

        // Datagram
        assert_eq!(&ntb[28..], &frame[..]);
    }

    #[test_case]
    fn build_ntb16_round_trips_with_parse_nth16() {
        let frame = [0u8; 60];
        let ntb = build_ntb16(&frame, 7);
        let nth = parse_nth16(&ntb).unwrap();
        assert_eq!(nth.sequence, 7);
        assert_eq!(nth.block_length as usize, ntb.len());
        assert_eq!(nth.ndp_index, 12);
    }

    #[test_case]
    fn iter_datagrams_single_via_build_ntb16() {
        let frame = [0xCDu8; 50];
        let ntb = build_ntb16(&frame, 0);
        let dgrams: alloc::vec::Vec<&[u8]> =
            iter_ntb16_datagrams(&ntb).collect();
        assert_eq!(dgrams.len(), 1);
        assert_eq!(dgrams[0], &frame[..]);
    }

    #[test_case]
    fn iter_datagrams_multi() {
        // Two 6-byte datagrams in a single NDP16.
        // Layout:
        //  0..12  : NTH16
        // 12..32  : NDP16 (NCM0 + len + next + 2 entries + (0,0) terminator)
        // 32..38  : datagram 0
        // 38..44  : datagram 1
        let mut ntb = alloc::vec![0u8; 44];
        // NTH16
        ntb[0..4].copy_from_slice(b"NCMH");
        ntb[4..6].copy_from_slice(&12u16.to_le_bytes());
        ntb[6..8].copy_from_slice(&0u16.to_le_bytes());
        ntb[8..10].copy_from_slice(&44u16.to_le_bytes());
        ntb[10..12].copy_from_slice(&12u16.to_le_bytes());
        // NDP16
        ntb[12..16].copy_from_slice(b"NCM0");
        ntb[16..18].copy_from_slice(&20u16.to_le_bytes()); // wLength
        ntb[18..20].copy_from_slice(&0u16.to_le_bytes()); // wNextNdpIndex
        ntb[20..22].copy_from_slice(&32u16.to_le_bytes()); // dgram0 idx
        ntb[22..24].copy_from_slice(&6u16.to_le_bytes()); // dgram0 len
        ntb[24..26].copy_from_slice(&38u16.to_le_bytes()); // dgram1 idx
        ntb[26..28].copy_from_slice(&6u16.to_le_bytes()); // dgram1 len

        // ntb[28..32] = (0,0) terminator (already zero)
        ntb[32..38].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        ntb[38..44].copy_from_slice(&[7, 8, 9, 10, 11, 12]);

        let dgrams: alloc::vec::Vec<&[u8]> =
            iter_ntb16_datagrams(&ntb).collect();
        assert_eq!(dgrams.len(), 2);
        assert_eq!(dgrams[0], &[1u8, 2, 3, 4, 5, 6][..]);
        assert_eq!(dgrams[1], &[7u8, 8, 9, 10, 11, 12][..]);
    }

    #[test_case]
    fn iter_datagrams_bad_ntb_yields_nothing() {
        let buf = [0u8; 12]; // valid byte count but no NCMH signature
        assert_eq!(iter_ntb16_datagrams(&buf).count(), 0);
    }
}
