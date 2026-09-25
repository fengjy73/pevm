//! Chase-Lev deque.
//!
//! The owner pushes and pops at the bottom (LIFO). Thieves pop the top
//! (FIFO). Indices follow the Chase–Lev protocol as corrected by Lê, Pop,
//! Cohen, and Zappa Nardelli: a release fence publishes the slot, and a
//! seqcst fence pairs the thief's top and bottom loads.
//! One owner; many thieves.

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering, fence};

#[repr(align(64))]
struct Bottom(AtomicIsize);

#[repr(align(64))]
struct Top(AtomicIsize);

pub(crate) struct LocalDeque {
    bottom: Bottom,
    top: Top,
    slots: Box<[AtomicUsize]>,
    mask: isize,
}

impl LocalDeque {
    pub(crate) fn with_capacity(min_live: usize) -> Self {
        let cap = (min_live.saturating_add(1))
            .next_power_of_two()
            .saturating_mul(2)
            .max(4);
        let slots = (0..cap).map(|_| AtomicUsize::new(0)).collect();
        Self {
            bottom: Bottom(AtomicIsize::new(0)),
            top: Top(AtomicIsize::new(0)),
            slots,
            mask: (cap as isize) - 1,
        }
    }

    #[inline]
    pub(crate) fn push_bottom(&self, tx: usize) {
        let b = self.bottom.0.load(Ordering::Relaxed);
        let t = self.top.0.load(Ordering::Acquire);
        let len = b.wrapping_sub(t);
        assert!(
            len <= self.mask,
            "deque overflow len={len} cap={}",
            self.mask + 1
        );
        self.slots[(b & self.mask) as usize].store(tx, Ordering::Relaxed);
        fence(Ordering::Release);
        self.bottom.0.store(b.wrapping_add(1), Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn pop_bottom(&self) -> Option<usize> {
        let b = self.bottom.0.load(Ordering::Relaxed).wrapping_sub(1);
        self.bottom.0.store(b, Ordering::Relaxed);
        fence(Ordering::SeqCst);
        let t = self.top.0.load(Ordering::Relaxed);
        if t > b {
            self.bottom.0.store(b.wrapping_add(1), Ordering::Relaxed);
            return None;
        }
        let tx = self.slots[(b & self.mask) as usize].load(Ordering::Relaxed);
        if t == b {
            let won = self
                .top
                .0
                .compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed)
                .is_ok();
            self.bottom.0.store(b.wrapping_add(1), Ordering::Relaxed);
            if !won {
                return None;
            }
        }
        Some(tx)
    }

    #[inline]
    pub(crate) fn pop_top(&self) -> Option<usize> {
        loop {
            let t = self.top.0.load(Ordering::Acquire);
            fence(Ordering::SeqCst);
            let b = self.bottom.0.load(Ordering::Acquire);
            if t >= b {
                return None;
            }
            let tx = self.slots[(t & self.mask) as usize].load(Ordering::Relaxed);
            if self
                .top
                .0
                .compare_exchange(t, t.wrapping_add(1), Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return Some(tx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    #[test]
    fn owner_lifo_thief_fifo() {
        let d = LocalDeque::with_capacity(8);
        d.push_bottom(1);
        d.push_bottom(2);
        d.push_bottom(3);
        assert_eq!(d.pop_bottom(), Some(3));
        assert_eq!(d.pop_top(), Some(1));
        assert_eq!(d.pop_bottom(), Some(2));
        assert_eq!(d.pop_bottom(), None);
    }

    #[test]
    fn thieves_take_each_once() {
        let d = Arc::new(LocalDeque::with_capacity(4096));
        let n = 500usize;
        for i in 0..n {
            d.push_bottom(i);
        }
        let stolen = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for _ in 0..3 {
            let d = Arc::clone(&d);
            let stolen = Arc::clone(&stolen);
            handles.push(thread::spawn(move || {
                let mut local = Vec::new();
                while let Some(tx) = d.pop_top() {
                    local.push(tx);
                }
                stolen.lock().unwrap().extend(local);
            }));
        }
        let mut all = Vec::new();
        while let Some(tx) = d.pop_bottom() {
            all.push(tx);
        }
        for h in handles {
            h.join().unwrap();
        }
        all.extend(stolen.lock().unwrap().iter().copied());
        all.sort_unstable();
        assert_eq!(all, (0..n).collect::<Vec<_>>());
    }
}
