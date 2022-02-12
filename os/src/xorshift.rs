pub struct Xorshift {
    x: u64,
}

impl Xorshift {
    pub fn init() -> Xorshift {
        Xorshift {
            x: 88172645463325252,
        }
    }
}

impl Iterator for Xorshift {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        self.x = self.x ^ (self.x << 7);
        self.x = self.x ^ (self.x >> 9);
        Some(self.x)
    }
}
