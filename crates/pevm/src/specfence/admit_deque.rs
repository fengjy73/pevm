//! Chase-Lev deque for one core's `AdmitIndep`.
//!
//! The owner pushes and pops at the bottom (LIFO). Thieves only pop the top
//! (FIFO). Indices use the weak-memory protocol from Chase and Lev, as
//! corrected by Lê, Pop, Cohen, and Zappa Nardelli: a release fence publishes
//! the slot, a seqcst fence pairs the thief's top and bottom loads.
//! Slots are relaxed atomics so the publication fences are the only
//! synchronization. One owner; many thieves. No mutex.

use std::fmt;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering, fence};

/// Owner-side index. Split from [`Top`] so a thief's CAS does not bounce
/// the owner's cache line.
#[repr(align(64))]
struct Bottom(AtomicIsize);

/// Thief-side index.
#[repr(align(64))]
struct Top(AtomicIsize);

/// Per-core `AdmitIndep` deque. `Sync` because every slot access is atomic and
/// the top/bottom protocol decides which side may touch a slot.
pub(crate) struct LocalAdmitDeque {
    bottom: Bottom,
    top: Top,
    slots: Box<[AtomicUsize]>,
    /// `capacity - 1`. Capacity is a power of two.
    mask: isize,
}

impl fmt::Debug for LocalAdmitDeque {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalAdmitDeque")
            .field("len", &self.len())
            .field("cap", &(self.mask + 1))
            .finish()
    }
}

impl LocalAdmitDeque {
    /// Capacity covers `min_live` simultaneous tasks with one spare slot
    /// so a full deque does not wrap onto a live index.
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
        // `mask` is cap-1. A push at `len == cap` would overwrite `top`.
        assert!(
            len <= self.mask,
            "AdmitIndep deque overflow len={len} cap={}",
            self.mask + 1
        );
        self.slots[(b & self.mask) as usize].store(tx, Ordering::Relaxed);
        fence(Ordering::Release);
        self.bottom.0.store(b.wrapping_add(1), Ordering::Relaxed);
    }

    /// Owner pop. `None` when this deque is empty.
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

    /// Thief pop. Retries when another thief wins the top CAS.
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

    #[inline]
    pub(crate) fn len(&self) -> usize {
        let b = self.bottom.0.load(Ordering::Acquire);
        let t = self.top.0.load(Ordering::Acquire);
        let len = b.wrapping_sub(t);
        if len > 0 { len as usize } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    #[test]
    fn owner_lifo_and_thief_fifo() {
        let d = LocalAdmitDeque::with_capacity(8);
        d.push_bottom(1);
        d.push_bottom(2);
        d.push_bottom(3);
        assert_eq!(d.pop_bottom(), Some(3));
        assert_eq!(d.pop_top(), Some(1));
        assert_eq!(d.pop_bottom(), Some(2));
        assert_eq!(d.pop_bottom(), None);
        assert_eq!(d.pop_top(), None);
        assert_eq!(d.len(), 0);
    }

    #[test]
    fn concurrent_thieves_take_each_task_once() {
        for _ in 0..20 {
            let d = Arc::new(LocalAdmitDeque::with_capacity(4096));
            let n = 2000usize;
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
                    stolen.lock().expect("stolen").extend(local);
                }));
            }
            let mut all = Vec::new();
            while let Some(tx) = d.pop_bottom() {
                all.push(tx);
            }
            for h in handles {
                h.join().expect("thief");
            }
            while let Some(tx) = d.pop_bottom() {
                all.push(tx);
            }
            all.extend(stolen.lock().expect("stolen").iter().copied());
            all.sort_unstable();
            assert_eq!(all.len(), n, "lost or duplicated task");
            for (i, tx) in all.iter().enumerate() {
                assert_eq!(*tx, i);
            }
        }
    }
}
