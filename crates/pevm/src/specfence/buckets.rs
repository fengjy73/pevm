//! Scoped buckets for the C=1 overhead split.
//!
//! Off unless `SPECFENCE_BUCKETS=1`. Wall-clock runs do not call `Instant::now`
//! from this module.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub(crate) const ALLOC: usize = 0;
pub(crate) const CLASS: usize = 1;
pub(crate) const PRESEED: usize = 2;
pub(crate) const RUNTIME: usize = 3;
pub(crate) const DEADLINE: usize = 4;
pub(crate) const COORD: usize = 5;
pub(crate) const MARK: usize = 6;
pub(crate) const PUBLISH: usize = 7;
pub(crate) const INTERP: usize = 8;
pub(crate) const RECORD: usize = 9;
pub(crate) const VALIDATE: usize = 10;
pub(crate) const RESCAN: usize = 11;
pub(crate) const LAZY: usize = 12;
pub(crate) const SCHED: usize = 13;
pub(crate) const TEMPLATE: usize = 14;
pub(crate) const PRE: usize = 15;
pub(crate) const WRITESET: usize = 16;
pub(crate) const N: usize = 17;

const NAMES: [&str; N] = [
    "alloc_chain",
    "class_key",
    "preseed",
    "runtime_seed",
    "deadline_clock",
    "coordinate",
    "mark_running",
    "publish",
    "interpreter",
    "mv_record",
    "validate",
    "rescan",
    "lazy_eval",
    "sched",
    "mv_template",
    "pre_interp",
    "writeset",
];

static ENABLED: OnceLock<bool> = OnceLock::new();
static NS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
static CALLS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

#[inline]
pub(crate) fn on() -> bool {
    *ENABLED.get_or_init(|| std::env::var("SPECFENCE_BUCKETS").ok().as_deref() == Some("1"))
}

pub(crate) struct Guard {
    bucket: usize,
    t0: Option<Instant>,
}

impl Guard {
    #[inline]
    pub(crate) fn start(bucket: usize) -> Self {
        Self {
            bucket,
            t0: on().then(Instant::now),
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let Some(t0) = self.t0 else {
            return;
        };
        NS[self.bucket].fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        CALLS[self.bucket].fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn dump() {
    if !on() {
        return;
    }
    eprint!("BUCKETS");
    for i in 0..N {
        let ns = NS[i].load(Ordering::Relaxed);
        let calls = CALLS[i].load(Ordering::Relaxed);
        eprint!(" {}={:.3}ms/{}", NAMES[i], ns as f64 / 1_000_000.0, calls);
        NS[i].store(0, Ordering::Relaxed);
        CALLS[i].store(0, Ordering::Relaxed);
    }
    eprintln!();
}
