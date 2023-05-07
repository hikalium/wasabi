#[repr(packed)]
#[allow(unused)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct InternetChecksum([u8; 2]);
impl InternetChecksum {
    pub fn calc(data: &[u8]) -> Self {
        // https://tools.ietf.org/html/rfc1071
        InternetChecksumGenerator::new().feed(data).checksum()
    }
}

// https://tools.ietf.org/html/rfc1071
#[derive(Copy, Clone, Default)]
struct InternetChecksumGenerator {
    sum: u32,
}
impl InternetChecksumGenerator {
    fn new() -> Self {
        Self::default()
    }
    fn feed(mut self, data: &[u8]) -> Self {
        let iter = data.chunks(2);
        for w in iter {
            self.sum += ((w[0] as u32) << 8) | w.get(1).cloned().unwrap_or_default() as u32;
        }
        self
    }
    fn checksum(mut self) -> InternetChecksum {
        while (self.sum >> 16) != 0 {
            self.sum = (self.sum & 0xffff) + (self.sum >> 16);
        }
        InternetChecksum((!self.sum as u16).to_be_bytes())
    }
}

#[test_case]
fn internet_checksum() {
    // https://datatracker.ietf.org/doc/html/rfc1071
    assert_eq!(
        InternetChecksumGenerator::new().checksum(),
        InternetChecksum([0xff, 0xff])
    );
    assert_eq!(
        InternetChecksumGenerator::new()
            .feed(&[
                0x00, 0x45, 0x73, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x40, 0x00, 0x00, 0xa8, 0xc0,
                0x01, 0x00, 0xa8, 0xc0, 0xc7, 0x00
            ])
            .checksum(),
        InternetChecksum([0x61, 0xb8])
    );
    assert_eq!(
        InternetChecksumGenerator::new()
            .feed(&[0x00, 0x45, 0x73, 0x00, 0x00, 0x00])
            .feed(&[0x00, 0x40, 0x11, 0x40, 0x00, 0x00, 0xa8, 0xc0])
            .feed(&[0x01, 0x00, 0xa8, 0xc0, 0xc7, 0x00])
            .checksum(),
        InternetChecksum([0x61, 0xb8])
    );
}
