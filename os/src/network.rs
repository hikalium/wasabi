use core::fmt;

pub struct EthernetAddress {
    mac: [u8; 6],
}
impl EthernetAddress {
    pub fn new(mac: &[u8; 6]) -> Self {
        EthernetAddress { mac: *mac }
    }
}
impl fmt::Display for EthernetAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:",
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5],
        )
    }
}
