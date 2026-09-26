//! Scoped buckets for the C=1 overhead split.
//!
//! Off unless `SPECFENCE_BUCKETS=1`. Wall-clock runs do not call `Instant::now`
//! from this module.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub(crate) const ALLOC: usize = 0;
pub(crate) const CLASS: usize = 1;
pub(crate) const PRESEED: usize = 2;
pub(crate) const RUNTIME: usize = 3;
pub(crate) const DEADLINE: usize = 4;
/// Slot 5 stays `coordinate` in the dump. Those waits now land in the wait buckets.
#[allow(dead_code)]
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
/// DashMap walk inside a tracked read. Nested in `interpreter`.
pub(crate) const READ_MV: usize = 17;
/// Read-origin map update inside a tracked read. Nested in `interpreter`.
pub(crate) const READ_ORIGIN: usize = 18;
/// Base-state fetch inside a tracked read. Nested in `interpreter`.
pub(crate) const READ_BASE: usize = 19;
/// Bytecode fetch inside a tracked read. Nested in `interpreter`.
pub(crate) const READ_CODE: usize = 20;
/// Interpreter time of plain transfers (no code on the callee).
pub(crate) const CLASS_PLAIN: usize = 21;
/// Interpreter time of the largest contract class.
pub(crate) const CLASS_HOT: usize = 22;
/// Interpreter time of every other transaction.
pub(crate) const CLASS_OTHER: usize = 23;
/// Interpreter time of incarnations above zero. Also counted in a class bucket.
pub(crate) const CLASS_REEXEC: usize = 24;
/// Tracked reads that skipped the directory and multi-version map.
pub(crate) const READ_COLD: usize = 25;
/// Armed-location coordination inside a read. Excluded from `interpreter`.
pub(crate) const WAIT_ARMED: usize = 26;
/// Estimate entry observed under the location lock. Excluded from `interpreter`.
pub(crate) const WAIT_ESTIMATE: usize = 27;
/// Unarmed chain coordination inside a read. Excluded from `interpreter`.
pub(crate) const WAIT_CHAIN: usize = 28;
/// Admission and nonce predecessor check. Excluded from `interpreter`.
pub(crate) const WAIT_ADMIT: usize = 29;
/// Shard or cache lock held by a read. Excluded from `interpreter`.
pub(crate) const WAIT_LOCK: usize = 30;
/// `Database` callback time: multi-version lookup, worker cache, storage fallback.
/// Excluded from `interpreter`, so that bucket is opcode time.
pub(crate) const DB_READ: usize = 31;
pub(crate) const N: usize = 32;

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
    "read_mv",
    "read_origin",
    "read_base",
    "read_code",
    "class_plain",
    "class_hot",
    "class_other",
    "class_reexec",
    "read_cold",
    "wait_armed",
    "wait_estimate",
    "wait_chain",
    "wait_admit",
    "wait_lock",
    "db_read",
];

static ENABLED: OnceLock<bool> = OnceLock::new();
static NS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
static CALLS: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

#[inline]
pub(crate) fn on() -> bool {
    *ENABLED.get_or_init(|| std::env::var("SPECFENCE_BUCKETS").ok().as_deref() == Some("1"))
}

/// Buckets or the inflation rank. Wall-clock runs leave both off, so wait
/// guards do not call `Instant::now`.
#[inline]
fn timing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| on() || std::env::var("SPECFENCE_INFLATION").ok().as_deref() == Some("1"))
}

thread_local! {
    static IN_INTERP: Cell<bool> = const { Cell::new(false) };
    static EXCLUDE: Cell<u64> = const { Cell::new(0) };
    static DB_DEPTH: Cell<u32> = const { Cell::new(0) };
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

/// Clock for a class split. `None` when buckets are off.
#[inline]
pub(crate) fn stamp() -> Option<Instant> {
    on().then(Instant::now)
}

/// Add a precomputed sample. Used when wait time has already been removed.
#[inline]
pub(crate) fn add_ns(bucket: usize, ns: u64) {
    if !on() {
        return;
    }
    NS[bucket].fetch_add(ns, Ordering::Relaxed);
    CALLS[bucket].fetch_add(1, Ordering::Relaxed);
}

/// Count a cold read without a nested clock.
#[inline]
pub(crate) fn hit(bucket: usize) {
    if on() {
        CALLS[bucket].fetch_add(1, Ordering::Relaxed);
    }
}

/// Time inside `handler.run` that is a wait, subtracted from `interpreter`.
pub(crate) struct WaitGuard {
    bucket: usize,
    t0: Option<Instant>,
}

impl WaitGuard {
    #[inline]
    pub(crate) fn start(bucket: usize) -> Self {
        Self {
            bucket,
            t0: timing().then(Instant::now),
        }
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        let Some(t0) = self.t0 else {
            return;
        };
        let ns = t0.elapsed().as_nanos() as u64;
        if on() {
            NS[self.bucket].fetch_add(ns, Ordering::Relaxed);
            CALLS[self.bucket].fetch_add(1, Ordering::Relaxed);
        }
        // Nested inside a database callback. That guard already excludes its
        // whole elapsed time, including this wait.
        if IN_INTERP.with(Cell::get) && DB_DEPTH.with(Cell::get) == 0 {
            EXCLUDE.with(|cell| cell.set(cell.get().saturating_add(ns)));
        }
    }
}

/// Time inside a `Database` method. The outermost guard is one `db_read` sample
/// and, during the interpreter, one exclusion. Inner waits do not exclude again.
pub(crate) struct DbGuard {
    t0: Option<Instant>,
}

impl DbGuard {
    #[inline]
    pub(crate) fn start() -> Self {
        if timing() {
            DB_DEPTH.with(|cell| cell.set(cell.get().saturating_add(1)));
        }
        Self {
            t0: timing().then(Instant::now),
        }
    }
}

impl Drop for DbGuard {
    fn drop(&mut self) {
        let Some(t0) = self.t0 else {
            return;
        };
        let ns = t0.elapsed().as_nanos() as u64;
        let depth = DB_DEPTH.with(|cell| {
            let depth = cell.get();
            cell.set(depth.saturating_sub(1));
            depth
        });
        if depth != 1 {
            return;
        }
        if on() {
            NS[DB_READ].fetch_add(ns, Ordering::Relaxed);
            CALLS[DB_READ].fetch_add(1, Ordering::Relaxed);
        }
        if IN_INTERP.with(Cell::get) {
            EXCLUDE.with(|cell| cell.set(cell.get().saturating_add(ns)));
        }
    }
}

/// Interpreter clock that does not include waits opened inside it.
pub(crate) struct InterpGuard {
    t0: Option<Instant>,
    finished: bool,
}

impl InterpGuard {
    #[inline]
    pub(crate) fn start() -> Self {
        if timing() {
            IN_INTERP.with(|cell| cell.set(true));
            EXCLUDE.with(|cell| cell.set(0));
        }
        Self {
            t0: timing().then(Instant::now),
            finished: false,
        }
    }

    /// Nanoseconds spent in wait guards. Also records the net interpreter bucket.
    pub(crate) fn finish(&mut self) -> u64 {
        if self.finished {
            return 0;
        }
        self.finished = true;
        IN_INTERP.with(|cell| cell.set(false));
        let excluded = EXCLUDE.with(|cell| cell.replace(0));
        let Some(t0) = self.t0.take() else {
            return 0;
        };
        let net = (t0.elapsed().as_nanos() as u64).saturating_sub(excluded);
        if on() {
            NS[INTERP].fetch_add(net, Ordering::Relaxed);
            CALLS[INTERP].fetch_add(1, Ordering::Relaxed);
        }
        excluded
    }
}

impl Drop for InterpGuard {
    fn drop(&mut self) {
        let _ = self.finish();
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
