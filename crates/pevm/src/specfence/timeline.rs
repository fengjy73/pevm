//! Per-worker timeline for the Stage 1c attribution.
//!
//! Off unless `SPECFENCE_TIMELINE=1`. Timed scans do not enter this module's
//! clock: hooks read a worker-local flag that is false for the whole block.

use std::cell::Cell;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use crate::TxIdx;

use super::live_chain::LiveChain;
use super::mv::SfMv;
use super::trace::Trace;

pub(crate) const EXEC: u8 = 1;
/// Kind 2. The interpreter no longer records an inline spin. The number stays
/// so older traces and `scripts/specfence_timeline_attrib.py` keep their mapping.
#[allow(dead_code)]
pub(crate) const INLINE: u8 = 2;
pub(crate) const VALIDATE: u8 = 3;
pub(crate) const IDLE: u8 = 4;
pub(crate) const SPIN: u8 = 5;
pub(crate) const PARK: u8 = 6;
pub(crate) const QUEUE: u8 = 7;
pub(crate) const POST: u8 = 8;

pub(crate) const ADMIT: u8 = 1;
pub(crate) const CLASS: u8 = 2;
pub(crate) const ARMED: u8 = 3;
pub(crate) const NONCE: u8 = 4;
pub(crate) const ESTIMATE: u8 = 5;
pub(crate) const UNARMED: u8 = 6;
pub(crate) const OTHER: u8 = 7;
pub(crate) const RESCAN: u8 = 8;
pub(crate) const LAZY: u8 = 9;
pub(crate) const SETUP: u8 = 10;
/// One drive-loop iteration. Covers the worker time outside interpreter spans.
pub(crate) const WORK: u8 = 11;

pub(crate) const CYC_COORD: usize = 0;
pub(crate) const CYC_SCHED: usize = 1;
pub(crate) const CYC_PUBLISH: usize = 2;
pub(crate) const CYC_RECORD: usize = 3;
pub(crate) const CYC_PRE: usize = 4;
pub(crate) const CYC_WRITE: usize = 5;
pub(crate) const CYC_MARK: usize = 6;
const CYC_N: usize = 7;

struct Span {
    kind: u8,
    reason: u8,
    tx: u32,
    pred: u32,
    loc: u64,
    class: u16,
    t0: u64,
    t1: u64,
}

struct OpenPark {
    t0: AtomicU64,
    wake: AtomicU64,
    pred: AtomicU32,
    class: AtomicU32,
    reason: AtomicU8,
    loc: AtomicU64,
    worker: AtomicU32,
}

struct Slot {
    spans: std::cell::UnsafeCell<Vec<Span>>,
}

unsafe impl Sync for Slot {}

struct Inner {
    base: Instant,
    n: usize,
    workers: usize,
    ns_per_cycle: f64,
    slots: Vec<Slot>,
    open: Vec<OpenPark>,
    exec_start: Vec<AtomicU64>,
    exec_end: Vec<AtomicU64>,
    commit_ns: Vec<AtomicU64>,
    exec_worker: Vec<AtomicU32>,
    cyc: Vec<AtomicU64>,
}

// Workers publish spans only into their own slot. The pointer is installed
// before those workers start and cleared after they join.
unsafe impl Sync for Inner {}

static ACTIVE: AtomicPtr<Inner> = AtomicPtr::new(std::ptr::null_mut());
static BLOCK_LABEL: AtomicU64 = AtomicU64::new(0);
/// Set for the whole block when a timeline is installed. `note_wake` runs on
/// the commit path, which does not consult the worker-local flag.
static INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TL_ON: Cell<bool> = const { Cell::new(false) };
    static WID: Cell<u32> = const { Cell::new(0) };
    static BLOCK: Cell<(u8, u64)> = const { Cell::new((OTHER, 0)) };
}

pub(crate) fn set_block_label(block: u64) {
    BLOCK_LABEL.store(block, Ordering::Relaxed);
}

fn flag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("SPECFENCE_TIMELINE").ok().as_deref(),
            Some("1" | "true" | "TRUE")
        )
    })
}

/// Bind this thread to the active timeline. Cheap no-op when the flag is off.
pub(crate) fn bind(worker: usize) {
    let on = flag() && !ACTIVE.load(Ordering::Acquire).is_null();
    TL_ON.with(|c| c.set(on));
    WID.with(|c| c.set(worker as u32));
}

#[inline]
fn on() -> bool {
    TL_ON.with(Cell::get)
}

#[inline]
fn inner() -> Option<&'static Inner> {
    let ptr = ACTIVE.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

#[inline]
fn worker() -> usize {
    WID.with(Cell::get) as usize
}

#[inline]
fn now_ns(base: Instant) -> u64 {
    base.elapsed().as_nanos() as u64
}

#[cfg(target_arch = "x86_64")]
fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(not(target_arch = "x86_64"))]
fn rdtsc() -> u64 {
    0
}

fn calibrate() -> f64 {
    let c0 = rdtsc();
    let t = Instant::now();
    while t.elapsed().as_millis() < 2 {
        std::hint::spin_loop();
    }
    let cycles = rdtsc().saturating_sub(c0).max(1);
    t.elapsed().as_nanos() as f64 / cycles as f64
}

pub(crate) struct Timeline {
    inner: Option<Box<Inner>>,
}

impl Timeline {
    pub(crate) fn start(n: usize, workers: usize) -> Self {
        if !flag() {
            return Self { inner: None };
        }
        let workers = workers.max(1);
        let ns_per_cycle = calibrate();
        let slots = (0..workers + 1)
            .map(|_| Slot {
                spans: std::cell::UnsafeCell::new(Vec::with_capacity(n.saturating_mul(2))),
            })
            .collect();
        let open = (0..n)
            .map(|_| OpenPark {
                t0: AtomicU64::new(0),
                wake: AtomicU64::new(0),
                pred: AtomicU32::new(u32::MAX),
                class: AtomicU32::new(u32::MAX),
                reason: AtomicU8::new(0),
                loc: AtomicU64::new(0),
                worker: AtomicU32::new(0),
            })
            .collect();
        let inner = Box::new(Inner {
            base: Instant::now(),
            n,
            workers,
            ns_per_cycle,
            slots,
            open,
            exec_start: (0..n).map(|_| AtomicU64::new(0)).collect(),
            exec_end: (0..n).map(|_| AtomicU64::new(0)).collect(),
            commit_ns: (0..n).map(|_| AtomicU64::new(0)).collect(),
            exec_worker: (0..n).map(|_| AtomicU32::new(u32::MAX)).collect(),
            cyc: (0..(workers + 1) * CYC_N)
                .map(|_| AtomicU64::new(0))
                .collect(),
        });
        let ptr = &*inner as *const Inner as *mut Inner;
        ACTIVE.store(ptr, Ordering::Release);
        INSTALLED.store(true, Ordering::Release);
        Self { inner: Some(inner) }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dump(
        &self,
        class_key: &str,
        full_replay: usize,
        reexec: usize,
        chain_len: usize,
        armed: usize,
        beneficiary: u64,
        live: &LiveChain,
        mv: &SfMv,
        prev_sender: &[Option<TxIdx>],
        trace: &Trace,
    ) {
        let Some(inner) = &self.inner else {
            return;
        };
        let path = std::env::var("SPECFENCE_TIMELINE_OUT").unwrap_or_default();
        if path.is_empty() {
            return;
        }
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let block = BLOCK_LABEL.load(Ordering::Relaxed);
        let mut file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                eprintln!("timeline open {path}: {err}");
                return;
            }
        };
        let wall = now_ns(inner.base);
        let _ = write!(
            file,
            "{{\"block\":{block},\"workers\":{},\"class\":\"{class_key}\",\"wall_ns\":{wall},\"n\":{},\"full_replay\":{full_replay},\"reexec\":{reexec},\"chain_len\":{chain_len},\"armed\":{armed},\"beneficiary\":{beneficiary},\"ns_per_cycle\":{:.6},\"exec_entries\":{},",
            inner.workers,
            inner.n,
            inner.ns_per_cycle,
            trace.exec_entries.load(Ordering::Relaxed),
        );
        let _ = write!(file, "\"cyc\":[");
        for (i, c) in inner.cyc.iter().enumerate() {
            if i > 0 {
                let _ = write!(file, ",");
            }
            let _ = write!(file, "{}", c.load(Ordering::Relaxed));
        }
        let _ = write!(file, "],\"txs\":[");
        for tx in 0..inner.n {
            if tx > 0 {
                let _ = write!(file, ",");
            }
            let class = live.class_id(tx);
            let _ = write!(
                file,
                "[{},{},{},{},{}]",
                inner.exec_start[tx].load(Ordering::Relaxed),
                inner.exec_end[tx].load(Ordering::Relaxed),
                inner.commit_ns[tx].load(Ordering::Relaxed),
                inner.exec_worker[tx].load(Ordering::Relaxed),
                class
            );
        }
        let _ = write!(file, "],\"spans\":[");
        let mut first = true;
        for (slot_i, slot) in inner.slots.iter().enumerate() {
            let spans = unsafe { &*slot.spans.get() };
            for span in spans {
                if !first {
                    let _ = write!(file, ",");
                }
                first = false;
                let _ = write!(
                    file,
                    "[{},{},{},{},{},{},{},{},{}]",
                    span.kind,
                    span.reason,
                    span.tx,
                    span.pred,
                    span.loc,
                    span.class,
                    span.t0,
                    span.t1,
                    slot_i
                );
            }
        }
        let mut reads: Vec<(usize, u64, u32)> = Vec::new();
        let mut writes: Vec<(usize, u64, u8)> = Vec::new();
        mv.visit_io(
            inner.n,
            |tx, loc, origin| reads.push((tx, loc, origin)),
            |tx, loc, lazy| writes.push((tx, loc, u8::from(lazy))),
        );
        let _ = write!(file, "],\"reads\":[");
        for (i, (tx, loc, origin)) in reads.iter().enumerate() {
            if i > 0 {
                let _ = write!(file, ",");
            }
            let _ = write!(file, "[{tx},{loc},{origin}]");
        }
        let _ = write!(file, "],\"writes\":[");
        for (i, (tx, loc, lazy)) in writes.iter().enumerate() {
            if i > 0 {
                let _ = write!(file, ",");
            }
            let _ = write!(file, "[{tx},{loc},{lazy}]");
        }
        let _ = write!(file, "],\"members\":[");
        let mut member_i = 0usize;
        live.visit_members(|loc, tx, kind| {
            if member_i > 0 {
                let _ = write!(file, ",");
            }
            member_i += 1;
            let _ = write!(file, "[{loc},{tx},{kind}]");
        });
        let _ = write!(
            file,
            "],\"delta_mismatch\":{},\"delta_abort\":{},\"sender\":[",
            trace.delta_mismatch.load(Ordering::Relaxed),
            trace.delta_abort.load(Ordering::Relaxed),
        );
        first = true;
        for (tx, prev) in prev_sender.iter().enumerate() {
            let Some(prev) = prev else {
                continue;
            };
            if !first {
                let _ = write!(file, ",");
            }
            first = false;
            let _ = write!(file, "[{prev},{tx}]");
        }
        let _ = writeln!(file, "]}}");
    }
}

impl Drop for Timeline {
    fn drop(&mut self) {
        INSTALLED.store(false, Ordering::Release);
        ACTIVE.store(std::ptr::null_mut(), Ordering::Release);
    }
}

fn push(kind: u8, reason: u8, tx: u32, pred: u32, loc: u64, class: u16, t0: u64, t1: u64) {
    let Some(inner) = inner() else {
        return;
    };
    let w = worker().min(inner.slots.len().saturating_sub(1));
    let spans = unsafe { &mut *inner.slots[w].spans.get() };
    spans.push(Span {
        kind,
        reason,
        tx,
        pred,
        loc,
        class,
        t0,
        t1,
    });
}

/// Start of one interpreter attempt. Drop records the span.
pub(crate) struct ExecSpan {
    tx: u32,
    t0: u64,
    pub(crate) done: bool,
}

impl ExecSpan {
    #[inline]
    pub(crate) const fn hot(&self) -> bool {
        self.t0 != 0
    }
}

pub(crate) struct CycGuard {
    bucket: usize,
    t0: u64,
}

impl CycGuard {
    #[inline]
    pub(crate) fn enter(enabled: bool, bucket: usize) -> Self {
        Self {
            bucket,
            t0: cyc_enter(enabled),
        }
    }
}

impl Drop for CycGuard {
    fn drop(&mut self) {
        cyc_leave(self.bucket, self.t0);
    }
}

impl ExecSpan {
    pub(crate) fn begin(tx: usize) -> Self {
        if !on() {
            return Self {
                tx: tx as u32,
                t0: 0,
                done: false,
            };
        }
        let Some(inner) = inner() else {
            return Self {
                tx: tx as u32,
                t0: 0,
                done: false,
            };
        };
        Self {
            tx: tx as u32,
            t0: now_ns(inner.base),
            done: false,
        }
    }
}

impl Drop for ExecSpan {
    fn drop(&mut self) {
        if self.t0 == 0 {
            return;
        }
        let Some(inner) = inner() else {
            return;
        };
        let t1 = now_ns(inner.base);
        if self.done {
            let tx = self.tx as usize;
            if tx < inner.n {
                inner.exec_start[tx].store(self.t0.saturating_add(1), Ordering::Relaxed);
                inner.exec_end[tx].store(t1.saturating_add(1), Ordering::Relaxed);
                inner.exec_worker[tx].store(worker() as u32, Ordering::Relaxed);
            }
        }
        push(EXEC, u8::from(self.done), self.tx, 0, 0, 0, self.t0, t1);
    }
}

#[inline]
pub(crate) fn set_block(reason: u8, loc: u64) {
    // The park path reads this even when the timeline dump is off.
    BLOCK.with(|c| c.set((reason, loc)));
}

pub(crate) fn block_wait() -> (u8, u64) {
    BLOCK.with(Cell::get)
}

pub(crate) fn open_park(tx: TxIdx, pred: TxIdx, loc: u64, class: u16, reason: u8) {
    if !on() {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    if tx >= inner.n {
        return;
    }
    let slot = &inner.open[tx];
    // A second opener keeps the first reason and start. `park` records every
    // wait; the caller that opened first already stored the precise class.
    if slot.t0.load(Ordering::Acquire) != 0 {
        return;
    }
    slot.pred.store(pred as u32, Ordering::Relaxed);
    slot.loc.store(loc, Ordering::Relaxed);
    slot.class.store(u32::from(class), Ordering::Relaxed);
    slot.reason.store(reason, Ordering::Relaxed);
    slot.worker.store(worker() as u32, Ordering::Relaxed);
    slot.wake.store(0, Ordering::Relaxed);
    slot.t0
        .store(now_ns(inner.base).saturating_add(1), Ordering::Release);
}

pub(crate) fn note_wake(tx: TxIdx) {
    if !INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    if tx >= inner.n {
        return;
    }
    let slot = &inner.open[tx];
    if slot.t0.load(Ordering::Acquire) == 0 {
        return;
    }
    let stamp = now_ns(inner.base).saturating_add(1);
    let _ = slot
        .wake
        .compare_exchange(0, stamp, Ordering::Release, Ordering::Relaxed);
}

pub(crate) fn close_park(tx: TxIdx) {
    if !on() {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    if tx >= inner.n {
        return;
    }
    let slot = &inner.open[tx];
    let t0 = slot.t0.swap(0, Ordering::AcqRel);
    if t0 == 0 {
        return;
    }
    let wake = slot.wake.swap(0, Ordering::Relaxed);
    let t1 = now_ns(inner.base);
    let pred = slot.pred.load(Ordering::Relaxed);
    let loc = slot.loc.load(Ordering::Relaxed);
    let class = slot.class.load(Ordering::Relaxed) as u16;
    let reason = slot.reason.load(Ordering::Relaxed);
    let start = t0 - 1;
    if wake > 1 {
        let woke = wake - 1;
        push(PARK, reason, tx as u32, pred, loc, class, start, woke);
        if t1 > woke {
            push(QUEUE, reason, tx as u32, pred, loc, class, woke, t1);
        }
    } else {
        push(PARK, reason, tx as u32, pred, loc, class, start, t1);
    }
}

pub(crate) fn stamp() -> u64 {
    if !on() {
        return 0;
    }
    inner().map(|inner| now_ns(inner.base)).unwrap_or(0)
}

pub(crate) fn validate_span(tx: TxIdx, t0: u64) {
    if t0 == 0 {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    push(VALIDATE, 0, tx as u32, 0, 0, 0, t0, now_ns(inner.base));
}

pub(crate) fn note_commit(tx: TxIdx) {
    if !on() {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    if tx < inner.n {
        inner.commit_ns[tx].store(now_ns(inner.base).saturating_add(1), Ordering::Relaxed);
    }
}

pub(crate) fn idle_span(t0: u64, waited: bool, ready: usize) {
    if t0 == 0 {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    // `pred` carries the ready-queue depth at the end of the idle span so a
    // commit-frontier gap can be told apart from a worker with nothing to run.
    push(
        if waited { IDLE } else { SPIN },
        u8::from(ready > 0),
        0,
        ready as u32,
        0,
        0,
        t0,
        now_ns(inner.base),
    );
}

/// Drive-loop coverage. Drop records a `POST`/`WORK` span on this worker.
pub(crate) struct WorkSpan {
    t0: u64,
}

impl WorkSpan {
    #[inline]
    pub(crate) fn begin() -> Self {
        Self {
            t0: if on() { stamp() } else { 0 },
        }
    }
}

impl Drop for WorkSpan {
    fn drop(&mut self) {
        if self.t0 == 0 {
            return;
        }
        let Some(inner) = inner() else {
            return;
        };
        push(POST, WORK, 0, 0, 0, 0, self.t0, now_ns(inner.base));
    }
}

pub(crate) fn post_span(reason: u8, t0: u64) {
    if t0 == 0 {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    let w = inner.workers;
    WID.with(|c| c.set(w as u32));
    TL_ON.with(|c| c.set(true));
    push(POST, reason, 0, 0, 0, 0, t0, now_ns(inner.base));
}

#[inline]
pub(crate) fn cyc_enter(enabled: bool) -> u64 {
    if !enabled {
        return 0;
    }
    rdtsc()
}

#[inline]
pub(crate) fn cyc_leave(bucket: usize, t0: u64) {
    if t0 == 0 {
        return;
    }
    let Some(inner) = inner() else {
        return;
    };
    let d = rdtsc().wrapping_sub(t0);
    let idx = worker()
        .min(inner.workers)
        .saturating_mul(CYC_N)
        .saturating_add(bucket);
    if let Some(slot) = inner.cyc.get(idx) {
        slot.fetch_add(d, Ordering::Relaxed);
    }
}

/// Worker-local view used by the VM so a timed run does not reload a global.
pub(crate) fn vm_enabled() -> bool {
    flag() && !ACTIVE.load(Ordering::Acquire).is_null()
}
