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
            self.sum += (hi << 8) | lo;
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
}
