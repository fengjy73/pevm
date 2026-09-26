//! Lock-free presence filter for location hashes.
//!
//! A set bit is a maybe. A clear pair is a no: the location was not inserted
//! before this load. Callers insert with release ordering before publishing
//! the value the filter stands for, and load with acquire before skipping
//! that structure.

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct LocBloom {
    bits: Box<[AtomicU64]>,
    mask: usize,
}

impl LocBloom {
    pub(crate) fn new(min_bits: usize) -> Self {
        let bits = min_bits.next_power_of_two().max(64);
        let words = bits / 64;
        Self {
            bits: (0..words).map(|_| AtomicU64::new(0)).collect(),
            mask: bits - 1,
        }
    }

    pub(crate) fn insert(&self, location: u64) {
        let (a, b) = indexes(location, self.mask);
        self.set(a);
        self.set(b);
    }

    pub(crate) fn may_contain(&self, location: u64) -> bool {
        let (a, b) = indexes(location, self.mask);
        self.get(a) && self.get(b)
    }

    fn set(&self, bit: usize) {
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        // A bit that is already set must not be written again. The hot
        // location is read on every core; a fetch_or would bounce that line.
        let current = self.bits[word].load(Ordering::Relaxed);
        if current & mask == 0 {
            self.bits[word].fetch_or(mask, Ordering::Release);
        }
    }

    fn get(&self, bit: usize) -> bool {
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        self.bits[word].load(Ordering::Acquire) & mask != 0
    }
}

fn indexes(location: u64, mask: usize) -> (usize, usize) {
    let h1 = location.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let h2 = h1.rotate_left(17) ^ location;
    ((h1 as usize) & mask, (h2 as usize) & mask)
}

#[cfg(test)]
mod tests {
    use super::LocBloom;

    #[test]
    fn insert_is_visible_and_absent_stays_clear() {
        let bloom = LocBloom::new(4096);
        assert!(!bloom.may_contain(0xabd6_bb39_7881_5b97));
        bloom.insert(0xabd6_bb39_7881_5b97);
        assert!(bloom.may_contain(0xabd6_bb39_7881_5b97));
        assert!(!bloom.may_contain(1));
    }
}
