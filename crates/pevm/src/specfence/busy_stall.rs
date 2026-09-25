//! Soft=0 busy / stall dig. Measurement only.
//!
//! Off unless `SPECFENCE_BUSY_STALL=1`. Flag-off call sites take the false
//! branch and do not read `Instant`. No scheduler, Avoid, Admit, or Learn
//! decision reads these counters.

use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub const KIND_EVM_FIRST: u8 = 0;
pub const KIND_EVM_FIRST_ABORT: u8 = 1;
pub const KIND_EVM_REFULL: u8 = 2;
pub const KIND_EVM_REPART: u8 = 3;
pub const KIND_VALIDATE: u8 = 4;
pub const KIND_PUBLISH: u8 = 5;
pub const KIND_SCHED: u8 = 6;
pub const KIND_DETECT: u8 = 7;
pub const KIND_LEARN: u8 = 8;
pub const KIND_LOCK: u8 = 9;
pub const KIND_SPIN: u8 = 10;
pub const KIND_STALL_HANDOFF: u8 = 11;
pub const KIND_STALL_COMMIT: u8 = 12;
pub const KIND_STALL_REFUSE: u8 = 13;
pub const KIND_STALL_WAITONCE: u8 = 14;
pub const KIND_STALL_NOREADY: u8 = 15;
pub const KIND_STALL_JOIN: u8 = 16;
pub const KIND_N: usize = 17;

pub const PRED_NONE: u32 = u32::MAX;
pub const BIN_NS: u64 = 250_000;
pub const BINS: usize = 200;
const WORKERS: usize = 16;
const SPANS_PER_WORKER: usize = 8192;

pub fn kind_name(k: u8) -> &'static str {
    match k {
        KIND_EVM_FIRST => "evm_first",
        KIND_EVM_FIRST_ABORT => "evm_first_abort",
        KIND_EVM_REFULL => "evm_refull",
        KIND_EVM_REPART => "evm_repart",
        KIND_VALIDATE => "validate",
        KIND_PUBLISH => "publish",
        KIND_SCHED => "sched",
        KIND_DETECT => "detect",
        KIND_LEARN => "learn",
        KIND_LOCK => "lock",
        KIND_SPIN => "spin",
        KIND_STALL_HANDOFF => "stall_handoff",
        KIND_STALL_COMMIT => "stall_commit",
        KIND_STALL_REFUSE => "stall_refuse",
        KIND_STALL_WAITONCE => "stall_waitonce",
        KIND_STALL_NOREADY => "stall_noready",
        KIND_STALL_JOIN => "stall_join",
        _ => "other",
    }
}

fn env_flag(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

/// Process-wide. The first read wins.
#[inline]
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_BUSY_STALL"))
}

thread_local! {
    static WID: Cell<u8> = const { Cell::new(0) };
    static NESTED: Cell<u64> = const { Cell::new(0) };
    static ORIGIN: Cell<Option<Instant>> = const { Cell::new(None) };
}

struct SpanRec {
    w: u8,
    k: u8,
    inc: u16,
    tx: u32,
    pred: u32,
    t0: u64,
    dur: u64,
    gross: u64,
}

struct Slot {
    ns: [AtomicU64; KIND_N],
    cnt: [AtomicU64; KIND_N],
    life: AtomicU64,
    dropped: AtomicU64,
    spans: Mutex<Vec<SpanRec>>,
}

struct Store {
    slots: [Slot; WORKERS],
    /// worker * KIND_N * BINS + kind * BINS + bin
    timeline: Box<[AtomicU64]>,
    overflow: AtomicU64,
    origin: Mutex<Option<Instant>>,
    sealed: Mutex<BusyStallSnap>,
}

fn store() -> &'static Store {
    static S: std::sync::OnceLock<Store> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let slots = std::array::from_fn(|_| Slot {
            ns: std::array::from_fn(|_| AtomicU64::new(0)),
            cnt: std::array::from_fn(|_| AtomicU64::new(0)),
            life: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            spans: Mutex::new(Vec::with_capacity(1024)),
        });
        let n = WORKERS * KIND_N * BINS;
        let timeline = (0..n).map(|_| AtomicU64::new(0)).collect::<Box<_>>();
        Store {
            slots,
            timeline,
            overflow: AtomicU64::new(0),
            origin: Mutex::new(None),
            sealed: Mutex::new(BusyStallSnap::disabled()),
        }
    })
}

#[derive(Clone, serde::Serialize)]
pub struct BusyStallSpan {
    pub w: u8,
    pub k: u8,
    pub kind: String,
    pub inc: u16,
    pub tx: u32,
    pub pred: Option<u32>,
    pub t0_ns: u64,
    pub dur_ns: u64,
    pub gross_ns: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct BusyStallWorker {
    pub worker: usize,
    pub life_ns: u64,
    pub ns: Vec<u64>,
    pub cnt: Vec<u64>,
    pub dropped_spans: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct BusyStallBin {
    pub w: u8,
    pub k: u8,
    pub bin: u16,
    pub ns: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct BusyStallSnap {
    pub enabled: bool,
    pub bin_ns: u64,
    pub bins: usize,
    pub kinds: Vec<String>,
    pub workers: Vec<BusyStallWorker>,
    /// Non-zero 0.25 ms bins only. `bin` is an index, not a timestamp.
    pub timeline: Vec<BusyStallBin>,
    pub timeline_overflow_ns: u64,
    pub spans: Vec<BusyStallSpan>,
}

impl BusyStallSnap {
    fn disabled() -> Self {
        Self {
            enabled: false,
            bin_ns: BIN_NS,
            bins: BINS,
            kinds: (0..KIND_N as u8).map(kind_name).map(str::to_string).collect(),
            workers: Vec::new(),
            timeline: Vec::new(),
            timeline_overflow_ns: 0,
            spans: Vec::new(),
        }
    }
}

/// Call on the host thread before workers spawn.
pub fn begin(origin: Instant) {
    if !enabled() {
        return;
    }
    let s = store();
    *s.origin.lock().unwrap() = Some(origin);
    s.overflow.store(0, Ordering::Relaxed);
    for slot in &s.slots {
        for k in 0..KIND_N {
            slot.ns[k].store(0, Ordering::Relaxed);
            slot.cnt[k].store(0, Ordering::Relaxed);
        }
        slot.life.store(0, Ordering::Relaxed);
        slot.dropped.store(0, Ordering::Relaxed);
        slot.spans.lock().unwrap().clear();
    }
    for b in s.timeline.iter() {
        b.store(0, Ordering::Relaxed);
    }
}

/// Call on the worker thread before it picks.
pub fn bind(worker_i: usize) {
    if !enabled() {
        return;
    }
    let w = worker_i.min(WORKERS - 1) as u8;
    WID.with(|c| c.set(w));
    NESTED.with(|c| c.set(0));
    let origin = *store().origin.lock().unwrap();
    ORIGIN.with(|c| c.set(origin));
}

pub fn worker_exit() {
    if !enabled() {
        return;
    }
    let Some(origin) = ORIGIN.with(|c| c.get()) else {
        return;
    };
    let life = origin.elapsed().as_nanos() as u64;
    let w = WID.with(|c| c.get()) as usize;
    store().slots[w].life.store(life, Ordering::Relaxed);
}

fn worker() -> usize {
    WID.with(|c| c.get()) as usize
}

fn since_origin(t: Instant) -> u64 {
    ORIGIN.with(|c| {
        c.get()
            .map(|o| t.saturating_duration_since(o).as_nanos() as u64)
            .unwrap_or(0)
    })
}

fn paint(w: usize, kind: u8, t0: u64, dur: u64) {
    if dur == 0 || kind as usize >= KIND_N {
        return;
    }
    let s = store();
    let mut left = dur;
    let mut t = t0;
    while left > 0 {
        let bin = (t / BIN_NS) as usize;
        if bin >= BINS {
            s.overflow.fetch_add(left, Ordering::Relaxed);
            break;
        }
        let bin_end = (bin as u64 + 1).saturating_mul(BIN_NS);
        let take = left.min(bin_end.saturating_sub(t)).max(1);
        let idx = (w * KIND_N + kind as usize) * BINS + bin;
        s.timeline[idx].fetch_add(take, Ordering::Relaxed);
        left = left.saturating_sub(take);
        t = t.saturating_add(take);
    }
}

fn bump(kind: u8, ns: u64, t0: u64) {
    if ns == 0 || kind as usize >= KIND_N {
        return;
    }
    let w = worker();
    let slot = &store().slots[w];
    slot.ns[kind as usize].fetch_add(ns, Ordering::Relaxed);
    slot.cnt[kind as usize].fetch_add(1, Ordering::Relaxed);
    paint(w, kind, t0, ns);
}

fn push_span(kind: u8, tx: u32, pred: u32, inc: u16, t0: u64, dur: u64, gross: u64) {
    if dur == 0 && gross == 0 {
        return;
    }
    let w = worker();
    let slot = &store().slots[w];
    let mut spans = slot.spans.lock().unwrap();
    if spans.len() >= SPANS_PER_WORKER {
        slot.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    spans.push(SpanRec {
        w: w as u8,
        k: kind,
        inc,
        tx,
        pred,
        t0,
        dur,
        gross,
    });
}

/// Bucket only. Does not touch the nested-stall counter.
#[inline]
pub fn charge(kind: u8, ns: u64, t0: Instant) {
    if !enabled() || ns == 0 {
        return;
    }
    bump(kind, ns, since_origin(t0));
}

/// Bucket + span. Does not touch the nested counter.
#[inline]
pub fn charge_span(kind: u8, ns: u64, t0: Instant, tx: u32, pred: u32, inc: u16, gross: u64) {
    if !enabled() || ns == 0 && gross == 0 {
        return;
    }
    let t0_ns = since_origin(t0);
    if ns > 0 {
        bump(kind, ns, t0_ns);
    }
    push_span(kind, tx, pred, inc, t0_ns, ns, gross);
}

/// Inside a parent interval (execute, pick, finish, detect). Parent subtracts
/// via [`take_nested`].
#[inline]
pub fn charge_nested(kind: u8, ns: u64, t0: Instant, tx: u32, pred: u32, inc: u16, span: bool) {
    if !enabled() || ns == 0 {
        return;
    }
    let t0_ns = since_origin(t0);
    bump(kind, ns, t0_ns);
    NESTED.with(|c| c.set(c.get().saturating_add(ns)));
    if span {
        push_span(kind, tx, pred, inc, t0_ns, ns, ns);
    }
}

#[inline]
pub fn take_nested() -> u64 {
    if !enabled() {
        return 0;
    }
    NESTED.with(|c| c.replace(0))
}

/// One `vm.execute` attempt. Inner stalls and detect are already on the nested
/// counter. `gross` on the span is the full attempt so a critical-path walk
/// can split out those inner stalls.
pub fn charge_exec(
    ok: bool,
    inc: usize,
    partial: bool,
    exec_ns: u64,
    t0: Instant,
    tx: usize,
    ordered_pred: Option<usize>,
    block_pred: Option<usize>,
) {
    if !enabled() {
        return;
    }
    let nested = take_nested();
    let mine = exec_ns.saturating_sub(nested);
    let kind = if !ok {
        if inc == 0 {
            KIND_EVM_FIRST_ABORT
        } else if partial {
            KIND_EVM_REPART
        } else {
            KIND_EVM_REFULL
        }
    } else if inc == 0 {
        KIND_EVM_FIRST
    } else if partial {
        KIND_EVM_REPART
    } else {
        KIND_EVM_REFULL
    };
    let pred = block_pred.or(ordered_pred);
    let t0_ns = since_origin(t0);
    if mine > 0 {
        bump(kind, mine, t0_ns);
    }
    push_span(
        kind,
        tx as u32,
        pred.map(|p| p as u32).unwrap_or(PRED_NONE),
        inc.min(u16::MAX as usize) as u16,
        t0_ns,
        mine,
        exec_ns,
    );
}

/// `finish_execution`. Status-lock acquire during the call is subtracted.
pub fn charge_publish(finish_ns: u64, t0: Instant, tx: usize, inc: usize) {
    if !enabled() {
        return;
    }
    let nested = take_nested();
    let mine = finish_ns.saturating_sub(nested);
    charge_span(
        KIND_PUBLISH,
        mine,
        t0,
        tx as u32,
        PRED_NONE,
        inc.min(u16::MAX as usize) as u16,
        finish_ns,
    );
}

/// Whole `pick`, minus nested lock / idle time already charged inside it.
#[inline]
pub fn charge_pick(ns: u64, t0: Instant) {
    if !enabled() {
        return;
    }
    let nested = take_nested();
    let mine = ns.saturating_sub(nested);
    if mine > 0 {
        bump(KIND_SCHED, mine, since_origin(t0));
    }
}

/// RAII interval. On drop, child nested time is subtracted and the remainder
/// is charged as `kind` (and added back onto the nested counter).
pub struct NestGuard {
    kind: u8,
    t0: Instant,
    nested0: u64,
    tx: u32,
    pred: u32,
    inc: u16,
    span: bool,
}

#[inline]
pub fn guard_at(
    kind: u8,
    t0: Instant,
    tx: usize,
    pred: usize,
    inc: u16,
    span: bool,
) -> Option<NestGuard> {
    if !enabled() {
        return None;
    }
    Some(NestGuard {
        kind,
        t0,
        nested0: NESTED.with(|c| c.get()),
        tx: tx as u32,
        pred: if pred == usize::MAX {
            PRED_NONE
        } else {
            pred as u32
        },
        inc,
        span,
    })
}

#[inline]
pub fn guard(kind: u8, tx: usize, pred: usize, inc: u16, span: bool) -> Option<NestGuard> {
    if !enabled() {
        return None;
    }
    guard_at(kind, Instant::now(), tx, pred, inc, span)
}

impl Drop for NestGuard {
    fn drop(&mut self) {
        let total = self.t0.elapsed().as_nanos() as u64;
        let child = NESTED.with(|c| c.get()).saturating_sub(self.nested0);
        let mine = total.saturating_sub(child);
        if mine == 0 {
            return;
        }
        let t0_ns = since_origin(self.t0);
        bump(self.kind, mine, t0_ns);
        NESTED.with(|c| c.set(c.get().saturating_add(mine)));
        if self.span {
            push_span(self.kind, self.tx, self.pred, self.inc, t0_ns, mine, mine);
        }
    }
}

/// Host thread, after workers join.
pub fn seal() {
    if !enabled() {
        let mut g = store().sealed.lock().unwrap();
        *g = BusyStallSnap::disabled();
        return;
    }
    let s = store();
    let mut workers = Vec::with_capacity(WORKERS);
    let mut spans = Vec::new();
    let mut n_workers = 0usize;
    for (i, slot) in s.slots.iter().enumerate() {
        let life = slot.life.load(Ordering::Relaxed);
        let ns: Vec<u64> = slot.ns.iter().map(|a| a.load(Ordering::Relaxed)).collect();
        let cnt: Vec<u64> = slot.cnt.iter().map(|a| a.load(Ordering::Relaxed)).collect();
        let dropped = slot.dropped.load(Ordering::Relaxed);
        if life == 0 && ns.iter().all(|n| *n == 0) {
            continue;
        }
        n_workers = n_workers.max(i + 1);
        workers.push(BusyStallWorker {
            worker: i,
            life_ns: life,
            ns,
            cnt,
            dropped_spans: dropped,
        });
        for rec in slot.spans.lock().unwrap().iter() {
            spans.push(BusyStallSpan {
                w: rec.w,
                k: rec.k,
                kind: kind_name(rec.k).to_string(),
                inc: rec.inc,
                tx: rec.tx,
                pred: if rec.pred == PRED_NONE {
                    None
                } else {
                    Some(rec.pred)
                },
                t0_ns: rec.t0,
                dur_ns: rec.dur,
                gross_ns: rec.gross,
            });
        }
    }
    let mut timeline = Vec::new();
    for w in 0..n_workers {
        for k in 0..KIND_N {
            for b in 0..BINS {
                let idx = (w * KIND_N + k) * BINS + b;
                let ns = s.timeline[idx].load(Ordering::Relaxed);
                if ns > 0 {
                    timeline.push(BusyStallBin {
                        w: w as u8,
                        k: k as u8,
                        bin: b as u16,
                        ns,
                    });
                }
            }
        }
    }
    let snap = BusyStallSnap {
        enabled: true,
        bin_ns: BIN_NS,
        bins: BINS,
        kinds: (0..KIND_N as u8).map(kind_name).map(str::to_string).collect(),
        workers,
        timeline,
        timeline_overflow_ns: s.overflow.load(Ordering::Relaxed),
        spans,
    };
    *s.sealed.lock().unwrap() = snap;
}

pub fn last_snap() -> BusyStallSnap {
    if !enabled() {
        return BusyStallSnap::disabled();
    }
    store().sealed.lock().unwrap().clone()
}

/// Status-mutex acquire. Hold time stays in the parent bucket.
#[inline]
pub fn time_lock_acq(t0: Option<Instant>) {
    let Some(t0) = t0 else {
        return;
    };
    let ns = t0.elapsed().as_nanos() as u64;
    charge_nested(KIND_LOCK, ns, t0, PRED_NONE, PRED_NONE, 0, false);
}

#[inline]
pub fn lock_t0() -> Option<Instant> {
    if enabled() {
        Some(Instant::now())
    } else {
        None
    }
}
