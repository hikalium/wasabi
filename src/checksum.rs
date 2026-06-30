// 16-bit one's-complement Internet Checksum, RFC 1071. Used by IPv4
// headers and ICMP packets.

#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct InternetChecksum([u8; 2]);
impl InternetChecksum {
    pub fn calc(data: &[u8]) -> Self {
        InternetChecksumGenerator::new().feed(data).checksum()
    }
    pub fn bytes(&self) -> [u8; 2] {
        self.0
    }
}

#[derive(Copy, Clone, Default)]
pub struct InternetChecksumGenerator {
    sum: u32,
}
impl InternetChecksumGenerator {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn feed(&mut self, data: &[u8]) -> &mut Self {
        for w in data.chunks(2) {
            let hi = w[0] as u32;
            let lo = w.get(1).cloned().unwrap_or_default() as u32;
            // `wrapping_add` + immediate carry-fold keeps `sum`
            // bounded so a multi-chunk feed (e.g. a >130 KiB TCP
            // retransmit) can't overflow `u32`. Without this the
            // generator panics on debug builds via the default
            // overflow check after ~65K chunks.
            let word = (hi << 8) | lo;
            let s = self.sum.wrapping_add(word);
            self.sum = (s & 0xffff) + (s >> 16);
        }
        self
    }
    pub fn checksum(&mut self) -> InternetChecksum {
        while (self.sum >> 16) != 0 {
            self.sum = (self.sum & 0xffff) + (self.sum >> 16);
        }
        InternetChecksum((!self.sum as u16).to_be_bytes())
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    // Worked examples from RFC 1071 §3.
    #[test_case]
    fn rfc1071_empty_is_all_ones() {
        assert_eq!(
            InternetChecksumGenerator::new().checksum(),
            InternetChecksum([0xff, 0xff])
        );
    }

    #[test_case]
    fn rfc1071_known_buffer() {
        let buf = [
            0x00, 0x45, 0x73, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x40, 0x00,
            0x00, 0xa8, 0xc0, 0x01, 0x00, 0xa8, 0xc0, 0xc7, 0x00,
        ];
        assert_eq!(
            InternetChecksum::calc(&buf),
            InternetChecksum([0x61, 0xb8])
        );
    }

    #[test_case]
    fn split_feed_matches_single_feed() {
        let mut a = InternetChecksumGenerator::new();
        a.feed(&[0x00, 0x45, 0x73, 0x00, 0x00, 0x00]);
        a.feed(&[0x00, 0x40, 0x11, 0x40, 0x00, 0x00, 0xa8, 0xc0]);
        a.feed(&[0x01, 0x00, 0xa8, 0xc0, 0xc7, 0x00]);
        assert_eq!(a.checksum(), InternetChecksum([0x61, 0xb8]));
    }

    /// A retransmit of a very large send_buffer (e.g. a wedged TCP
    /// connection where unacked data piled up) hands a multi-hundred
    /// kilobyte segment to `tcp_segment_checksum`. Make sure the
    /// generator doesn't overflow its u32 accumulator on the way.
    #[test_case]
    fn feed_large_buffer_does_not_overflow() {
        // 256 KiB of 0xFF — every 2-byte chunk contributes 0xFFFF,
        // which is the worst-case input for the accumulator.
        let big = alloc::vec![0xFFu8; 256 * 1024];
        // The point of the test is "doesn't panic"; calling
        // `checksum()` also exercises the final fold loop.
        let _ = InternetChecksumGenerator::new().feed(&big).checksum();
    }
}
