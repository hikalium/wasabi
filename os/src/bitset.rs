/// A simple set of bits
/// const type parameter N represents how many bytes it uses.
/// 0..(N*8) will be the range of values.
#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub enum Error {
    OutOfRange,
}
type Result<T> = core::result::Result<T, Error>;
#[derive(Debug, Copy, Clone)]
pub struct BitSet<const N: usize> {
    bytes: [u8; N],
}
impl<const N: usize> BitSet<N> {
    pub fn new() -> Self {
        Self { bytes: [0u8; N] }
    }
    pub fn insert(&mut self, index: usize) -> Result<()> {
        if index < N * 8 {
            self.bytes[index / 8] |= 1u8 << (index % 8);
            Ok(())
        } else {
            Err(Error::OutOfRange)
        }
    }
    pub fn get(&self, index: usize) -> Result<bool> {
        if index < N * 8 {
            Ok((self.bytes[index / 8] & (1u8 << (index % 8))) != 0)
        } else {
            Err(Error::OutOfRange)
        }
    }
    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index < N * 8 {
            self.bytes[index / 8] &= !(1u8 << (index % 8));
            Ok(())
        } else {
            Err(Error::OutOfRange)
        }
    }
    pub fn clear(&mut self) {
        self.bytes.iter_mut().for_each(|v| *v = 0);
    }
    pub fn symmetric_difference(&self, another: &Self) -> Self {
        let mut res = Self::new();
        for i in 0..N {
            res.bytes[i] = self.bytes[i] ^ another.bytes[i];
        }
        res
    }
    pub fn iter(&self) -> BitSetIterator<N> {
        BitSetIterator {
            set: *self,
            range: 0..N * 8,
        }
    }
}
impl<const N: usize> Default for BitSet<N> {
    fn default() -> Self {
        Self { bytes: [0u8; N] }
    }
}

pub struct BitSetIterator<const N: usize> {
    set: BitSet<N>,
    range: core::ops::Range<usize>,
}
impl<const N: usize> Iterator for BitSetIterator<N> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        self.range.find(|i| self.set.get(*i).unwrap())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test_case]
    fn create() {
        let b = BitSet::<3>::new();
        for i in 0..24 {
            assert_eq!(b.get(i), Ok(false));
        }
        assert!(b.get(24).is_err());
    }
    #[test_case]
    fn insert_get() {
        let mut b = BitSet::<3>::new();
        assert!(b.insert(0).is_ok());
        assert!(b.insert(1).is_ok());
        assert!(b.insert(5).is_ok());
        assert!(b.insert(9).is_ok());
        assert!(b.insert(23).is_ok());
        assert!(b.insert(24).is_err());

        for i in 0..24 {
            assert_eq!(b.get(i), Ok(matches!(i, 0 | 1 | 5 | 9 | 23)));
        }
        assert!(b.get(24).is_err());
    }
    #[test_case]
    fn insert_remove_get() {
        let mut b = BitSet::<3>::new();
        assert!(b.insert(0).is_ok());
        assert!(b.insert(1).is_ok());
        assert!(b.insert(5).is_ok());
        assert!(b.insert(9).is_ok());
        assert!(b.insert(23).is_ok());
        assert!(b.insert(24).is_err());
        assert!(b.remove(0).is_ok());
        assert!(b.remove(1).is_ok());
        assert!(b.remove(2).is_ok());
        assert!(b.remove(9).is_ok());
        assert!(b.remove(24).is_err());

        for i in 0..24 {
            assert_eq!(b.get(i), Ok(matches!(i, 5 | 23)));
        }
        assert!(b.get(24).is_err());
    }
    #[test_case]
    fn insert_clear() {
        let mut b = BitSet::<3>::new();
        assert!(b.insert(0).is_ok());
        assert!(b.insert(1).is_ok());
        assert!(b.insert(5).is_ok());
        assert!(b.insert(9).is_ok());
        assert!(b.insert(23).is_ok());
        assert!(b.insert(24).is_err());
        b.clear();

        for i in 0..24 {
            assert_eq!(b.get(i), Ok(false));
        }
        assert!(b.get(24).is_err());
    }
    #[test_case]
    fn symmetric_difference() {
        let mut a = BitSet::<3>::new();
        assert!(a.insert(0).is_ok());
        assert!(a.insert(1).is_ok());
        assert!(a.insert(2).is_ok());
        assert!(a.insert(3).is_ok());
        let mut b = BitSet::<3>::new();
        assert!(b.insert(0).is_ok());
        assert!(b.insert(1).is_ok());
        assert!(b.insert(5).is_ok());
        assert!(b.insert(9).is_ok());
        assert!(b.insert(23).is_ok());
        let c = a.symmetric_difference(&b);
        for i in 0..24 {
            assert_eq!(c.get(i), Ok(matches!(i, 2 | 3 | 5 | 9 | 23)));
        }
    }
    #[test_case]
    fn iter() {
        let mut b = BitSet::<3>::new();
        assert!(b.insert(0).is_ok());
        assert!(b.insert(1).is_ok());
        assert!(b.insert(5).is_ok());
        assert!(b.insert(9).is_ok());
        let mut it = b.iter();
        assert_eq!(it.next(), Some(0));
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next(), Some(5));
        assert_eq!(it.next(), Some(9));
        assert_eq!(it.next(), None);
        assert_eq!(it.next(), None);
    }
}
