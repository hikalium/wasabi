const PAGE_OFFSET_BITS: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_OFFSET_BITS;

pub fn size_in_pages_from_bytes(size_in_bytes: usize) -> usize {
    (size_in_bytes + PAGE_SIZE - 1) >> PAGE_OFFSET_BITS
}
