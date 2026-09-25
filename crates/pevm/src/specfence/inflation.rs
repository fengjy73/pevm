#![allow(missing_docs)]
//! Per-tx execute inflation probe. Measurement only.
//!
//! Off unless `SPECFENCE_INFLATION=1`. Flag-off call sites do not read
//! `Instant`. No scheduler, Avoid, Admit, or Learn decision reads these
//! counters.
//!
//! Extra flags (each implies the rows are labeled, not product wall):
//! - `SPECFENCE_INFLATION_READS=1` — per-read MvMemory vs storage `Instant`
//! - `SPECFENCE_INFLATION_OS=1` — `getrusage(RUSAGE_THREAD)` switches
//! - `SPECFENCE_INFLATION_PERF=1` — `perf_event_open` instructions / misses
//! - `SPECFENCE_INFLATION_DAG=1` — keep final read/write hashes
//! - `SPECFENCE_INFLATION_ALLOC=1` — allocation counts, only if the process
//!   installed [`InflationAlloc`]

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::Serialize;

pub const TAG_OCC: u8 = 1;
pub const TAG_SF: u8 = 2;
pub const TAG_SEQ_VM: u8 = 3;
pub const TAG_SEQ_TX: u8 = 4;

fn env_flag(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

#[inline]
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_INFLATION"))
}

#[inline]
pub fn reads_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_INFLATION_READS"))
}

#[inline]
fn os_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_INFLATION_OS"))
}

#[inline]
fn perf_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_INFLATION_PERF"))
}

#[inline]
fn dag_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_INFLATION_DAG"))
}

#[inline]
fn alloc_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| env_flag("SPECFENCE_INFLATION_ALLOC"))
}

static TAG: AtomicU8 = AtomicU8::new(0);
static BENEFICIARY: AtomicU64 = AtomicU64::new(0);

pub fn set_tag(tag: u8) {
    TAG.store(tag, Ordering::Relaxed);
}

pub fn note_beneficiary(hash: u64) {
    if enabled() {
        BENEFICIARY.store(hash, Ordering::Relaxed);
    }
}

#[derive(Clone, Serialize)]
pub struct Attempt {
    pub tag: u8,
    pub tx: u32,
    pub inc: u16,
    pub kind: u8,
    pub pre_ns: u64,
    pub interp_ns: u64,
    pub post_ns: u64,
    pub total_ns: u64,
    pub cpu_ns: u64,
    pub opcode_ns: u64,
    pub vmdb_ns: u64,
    pub detect_ns: u64,
    pub split: bool,
    pub record_ns: u64,
    pub mv_lookup_ns: u64,
    pub mv_scan_ns: u64,
    pub storage_ns: u64,
    pub account_n: u32,
    pub sload_n: u32,
    pub storage_n: u32,
    pub lazy_n: u32,
    pub nvcsw: i64,
    pub nivcsw: i64,
    pub instr: u64,
    pub cache_miss: u64,
    pub llc_miss: u64,
    pub perf_ok: bool,
    pub alloc_n: u64,
    pub alloc_b: u64,
    pub gas_used: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lazy_writes: Vec<u64>,
}

#[derive(Clone, Serialize)]
pub struct SeqTx {
    pub tx: u32,
    pub transact_ns: u64,
    pub commit_ns: u64,
    pub cpu_ns: u64,
}

#[derive(Clone, Serialize)]
pub struct ValNote {
    pub tag: u8,
    pub tx: u32,
    pub inc: u16,
    pub ns: u64,
}

pub struct InflationDrain {
    pub attempts: Vec<Attempt>,
    pub seq: Vec<SeqTx>,
    pub vals: Vec<ValNote>,
    pub beneficiary: u64,
    pub boundary: BoundarySnap,
}

struct Store {
    attempts: Mutex<Vec<Attempt>>,
    seq: Mutex<Vec<SeqTx>>,
    vals: Mutex<Vec<ValNote>>,
}

fn store() -> &'static Store {
    static S: OnceLock<Store> = OnceLock::new();
    S.get_or_init(|| Store {
        attempts: Mutex::new(Vec::with_capacity(2048)),
        seq: Mutex::new(Vec::with_capacity(2048)),
        vals: Mutex::new(Vec::with_capacity(2048)),
    })
}

struct Acc {
    mv_lookup_ns: u64,
    mv_scan_ns: u64,
    storage_ns: u64,
    account_n: u32,
    sload_n: u32,
    storage_n: u32,
    lazy_n: u32,
    record_ns: u64,
    gas_used: u64,
    reads: Vec<u64>,
    writes: Vec<u64>,
    lazy_writes: Vec<u64>,
    cpu0: u64,
    nvcsw0: i64,
    nivcsw0: i64,
    instr0: u64,
    miss0: u64,
    llc0: u64,
    perf_ok: bool,
    alloc_n0: u64,
    alloc_b0: u64,
}

impl Acc {
    fn clear(&mut self) {
        self.mv_lookup_ns = 0;
        self.mv_scan_ns = 0;
        self.storage_ns = 0;
        self.account_n = 0;
        self.sload_n = 0;
        self.storage_n = 0;
        self.lazy_n = 0;
        self.record_ns = 0;
        self.gas_used = 0;
        self.reads.clear();
        self.writes.clear();
        self.lazy_writes.clear();
        self.cpu0 = 0;
        self.nvcsw0 = 0;
        self.nivcsw0 = 0;
        self.instr0 = 0;
        self.miss0 = 0;
        self.llc0 = 0;
        self.perf_ok = false;
        self.alloc_n0 = 0;
        self.alloc_b0 = 0;
    }
}

thread_local! {
    static ACC: std::cell::RefCell<Acc> = std::cell::RefCell::new(Acc {
        mv_lookup_ns: 0,
        mv_scan_ns: 0,
        storage_ns: 0,
        account_n: 0,
        sload_n: 0,
        storage_n: 0,
        lazy_n: 0,
        record_ns: 0,
        gas_used: 0,
        reads: Vec::new(),
        writes: Vec::new(),
        lazy_writes: Vec::new(),
        cpu0: 0,
        nvcsw0: 0,
        nivcsw0: 0,
        instr0: 0,
        miss0: 0,
        llc0: 0,
        perf_ok: false,
        alloc_n0: 0,
        alloc_b0: 0,
    });
}

// `gas_used` lives on Acc; the initializer above includes it.

pub(crate) fn begin_attempt() {
    if !enabled() {
        return;
    }
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.clear();
        a.cpu0 = thread_cpu_ns();
        if os_on() {
            let (v, n) = thread_switches();
            a.nvcsw0 = v;
            a.nivcsw0 = n;
        }
        if perf_on() {
            let (ok, instr, miss, llc) = perf_read();
            a.perf_ok = ok;
            a.instr0 = instr;
            a.miss0 = miss;
            a.llc0 = llc;
        }
        if alloc_on() {
            let (n, b) = alloc_now();
            a.alloc_n0 = n;
            a.alloc_b0 = b;
        }
    });
}

pub(crate) struct ExecCut {
    pub tx: usize,
    pub inc: u16,
    pub kind: u8,
    pub pre_ns: u64,
    pub interp_ns: u64,
    pub post_ns: u64,
    pub total_ns: u64,
    pub opcode_ns: u64,
    pub vmdb_ns: u64,
    pub detect_ns: u64,
    pub split: bool,
}

pub(crate) fn commit_exec(cut: ExecCut) {
    if !enabled() {
        return;
    }
    let row = ACC.with(|a| {
        let a = a.borrow();
        let cpu1 = thread_cpu_ns();
        let (nvcsw, nivcsw) = if os_on() {
            let (v, n) = thread_switches();
            (v.saturating_sub(a.nvcsw0), n.saturating_sub(a.nivcsw0))
        } else {
            (0, 0)
        };
        let (perf_ok, instr, miss, llc) = if perf_on() {
            let (ok, i, m, l) = perf_read();
            (
                ok && a.perf_ok,
                i.saturating_sub(a.instr0),
                m.saturating_sub(a.miss0),
                l.saturating_sub(a.llc0),
            )
        } else {
            (false, 0, 0, 0)
        };
        let (alloc_n, alloc_b) = if alloc_on() {
            let (n, b) = alloc_now();
            (n.saturating_sub(a.alloc_n0), b.saturating_sub(a.alloc_b0))
        } else {
            (0, 0)
        };
        Attempt {
            tag: TAG.load(Ordering::Relaxed),
            tx: cut.tx as u32,
            inc: cut.inc,
            kind: cut.kind,
            pre_ns: cut.pre_ns,
            interp_ns: cut.interp_ns,
            post_ns: cut.post_ns,
            total_ns: cut.total_ns,
            cpu_ns: cpu1.saturating_sub(a.cpu0),
            opcode_ns: cut.opcode_ns,
            vmdb_ns: cut.vmdb_ns,
            detect_ns: cut.detect_ns,
            split: cut.split,
            record_ns: a.record_ns,
            mv_lookup_ns: a.mv_lookup_ns,
            mv_scan_ns: a.mv_scan_ns,
            storage_ns: a.storage_ns,
            account_n: a.account_n,
            sload_n: a.sload_n,
            storage_n: a.storage_n,
            lazy_n: a.lazy_n,
            nvcsw,
            nivcsw,
            instr,
            cache_miss: miss,
            llc_miss: llc,
            perf_ok,
            alloc_n,
            alloc_b,
            gas_used: a.gas_used,
            reads: a.reads.clone(),
            writes: a.writes.clone(),
            lazy_writes: a.lazy_writes.clone(),
        }
    });
    if let Ok(mut g) = store().attempts.lock() {
        g.push(row);
    }
}

pub(crate) fn note_gas(gas: u64) {
    if !enabled() {
        return;
    }
    ACC.with(|a| a.borrow_mut().gas_used = gas);
}

pub(crate) fn note_record(ns: u64) {
    if !enabled() {
        return;
    }
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.record_ns = a.record_ns.saturating_add(ns);
    });
}

pub(crate) fn note_rw(reads: impl Iterator<Item = u64>, writes: impl Iterator<Item = (u64, bool)>) {
    if !enabled() || !dag_on() {
        return;
    }
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.reads.clear();
        a.writes.clear();
        a.lazy_writes.clear();
        a.reads.extend(reads);
        for (h, lazy) in writes {
            if lazy {
                a.lazy_writes.push(h);
            } else {
                a.writes.push(h);
            }
        }
    });
}

#[inline]
pub(crate) fn add_lookup(ns: u64) {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.mv_lookup_ns = a.mv_lookup_ns.saturating_add(ns);
    });
}

#[inline]
pub(crate) fn add_scan(ns: u64) {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.mv_scan_ns = a.mv_scan_ns.saturating_add(ns);
    });
}

#[inline]
pub(crate) fn add_storage(ns: u64) {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.storage_ns = a.storage_ns.saturating_add(ns);
        a.storage_n = a.storage_n.saturating_add(1);
    });
}

#[inline]
pub(crate) fn add_account() {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.account_n = a.account_n.saturating_add(1);
    });
}

#[inline]
pub(crate) fn add_sload() {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.sload_n = a.sload_n.saturating_add(1);
    });
}

#[inline]
pub(crate) fn add_lazy() {
    ACC.with(|a| {
        let mut a = a.borrow_mut();
        a.lazy_n = a.lazy_n.saturating_add(1);
    });
}

pub(crate) fn note_val(tx: usize, inc: u16, ns: u64) {
    if !enabled() {
        return;
    }
    if let Ok(mut g) = store().vals.lock() {
        g.push(ValNote {
            tag: TAG.load(Ordering::Relaxed),
            tx: tx as u32,
            inc,
            ns,
        });
    }
}

pub(crate) fn note_seq(tx: u32, transact_ns: u64, commit_ns: u64, cpu_ns: u64) {
    if !enabled() {
        return;
    }
    if let Ok(mut g) = store().seq.lock() {
        g.push(SeqTx {
            tx,
            transact_ns,
            commit_ns,
            cpu_ns,
        });
    }
}

#[derive(Clone, Serialize)]
pub struct BoundarySnap {
    pub enter_to_return_ns: u64,
    pub f_pre_ns: u64,
    pub f_post_ns: u64,
    pub worker_seen: bool,
}

struct BoundState {
    enter: Mutex<Option<Instant>>,
    worker_lo: AtomicU64,
    worker_hi: AtomicU64,
    ret_ns: AtomicU64,
}

fn bound_state() -> &'static BoundState {
    static B: OnceLock<BoundState> = OnceLock::new();
    B.get_or_init(|| BoundState {
        enter: Mutex::new(None),
        worker_lo: AtomicU64::new(u64::MAX),
        worker_hi: AtomicU64::new(0),
        ret_ns: AtomicU64::new(0),
    })
}

pub struct BoundGuard {
    active: bool,
}

impl BoundGuard {
    pub fn enter() -> Self {
        if !enabled() {
            return Self { active: false };
        }
        let b = bound_state();
        if let Ok(mut g) = b.enter.lock() {
            *g = Some(Instant::now());
        }
        b.worker_lo.store(u64::MAX, Ordering::Relaxed);
        b.worker_hi.store(0, Ordering::Relaxed);
        b.ret_ns.store(0, Ordering::Relaxed);
        Self { active: true }
    }
}

impl Drop for BoundGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let b = bound_state();
        let ns = b
            .enter
            .lock()
            .ok()
            .and_then(|g| g.map(|t| t.elapsed().as_nanos() as u64))
            .unwrap_or(0);
        b.ret_ns.store(ns, Ordering::Relaxed);
    }
}

pub fn mark_worker_enter() {
    if !enabled() {
        return;
    }
    let b = bound_state();
    let Some(t0) = b.enter.lock().ok().and_then(|g| *g) else {
        return;
    };
    let ns = t0.elapsed().as_nanos() as u64;
    let mut cur = b.worker_lo.load(Ordering::Relaxed);
    while ns < cur {
        match b
            .worker_lo
            .compare_exchange_weak(cur, ns, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(v) => cur = v,
        }
    }
}

/// Pin this worker to `SPECFENCE_PIN_CPUS` entry `worker_i` (modulo).
///
/// Empty or unset env is a no-op, so product runs do not change affinity.
/// The scan script sets one distinct physical cpu per worker. An
/// oversubscription contrast may list fewer cpus than workers; extra
/// workers share via modulo.
pub fn pin_worker(worker_i: usize) {
    let cpus = pin_cpus();
    if cpus.is_empty() {
        return;
    }
    let cpu = cpus[worker_i % cpus.len()];
    if cpu >= 1024 {
        return;
    }
    let mut mask = [0u8; 128];
    mask[cpu / 8] = 1u8 << (cpu % 8);
    unsafe {
        sched_setaffinity(0, mask.len(), mask.as_ptr());
    }
}

fn pin_cpus() -> &'static [usize] {
    static CPUS: OnceLock<Vec<usize>> = OnceLock::new();
    CPUS.get_or_init(|| {
        std::env::var("SPECFENCE_PIN_CPUS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|p| p.trim().parse::<usize>().ok())
                    .collect()
            })
            .unwrap_or_default()
    })
    .as_slice()
}

pub fn mark_worker_exit() {
    if !enabled() {
        return;
    }
    let b = bound_state();
    let Some(t0) = b.enter.lock().ok().and_then(|g| *g) else {
        return;
    };
    let ns = t0.elapsed().as_nanos() as u64;
    let mut cur = b.worker_hi.load(Ordering::Relaxed);
    while ns > cur {
        match b
            .worker_hi
            .compare_exchange_weak(cur, ns, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(v) => cur = v,
        }
    }
}

fn boundary_snap() -> BoundarySnap {
    if !enabled() {
        return BoundarySnap {
            enter_to_return_ns: 0,
            f_pre_ns: 0,
            f_post_ns: 0,
            worker_seen: false,
        };
    }
    let b = bound_state();
    let ret = b.ret_ns.load(Ordering::Relaxed);
    let lo = b.worker_lo.load(Ordering::Relaxed);
    let hi = b.worker_hi.load(Ordering::Relaxed);
    let seen = lo != u64::MAX;
    BoundarySnap {
        enter_to_return_ns: ret,
        f_pre_ns: if seen { lo } else { 0 },
        f_post_ns: if seen { ret.saturating_sub(hi) } else { 0 },
        worker_seen: seen,
    }
}

pub fn drain() -> InflationDrain {
    let attempts = store()
        .attempts
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    let seq = store()
        .seq
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    let vals = store()
        .vals
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    InflationDrain {
        attempts,
        seq,
        vals,
        beneficiary: BENEFICIARY.load(Ordering::Relaxed),
        boundary: boundary_snap(),
    }
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
struct Rusage {
    ru_utime: Timeval,
    ru_stime: Timeval,
    ru_maxrss: i64,
    ru_ixrss: i64,
    ru_idrss: i64,
    ru_isrss: i64,
    ru_minflt: i64,
    ru_majflt: i64,
    ru_nswap: i64,
    ru_inblock: i64,
    ru_oublock: i64,
    ru_msgsnd: i64,
    ru_msgrcv: i64,
    ru_nsignals: i64,
    ru_nvcsw: i64,
    ru_nivcsw: i64,
}

unsafe extern "C" {
    fn clock_gettime(clk_id: i32, tp: *mut Timespec) -> i32;
    fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    fn syscall(nr: i64, ...) -> i64;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const u8) -> i32;
}

const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
const RUSAGE_THREAD: i32 = 1;

pub(crate) fn thread_cpu_ns() -> u64 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if rc != 0 {
        return 0;
    }
    (ts.tv_sec as u64).saturating_mul(1_000_000_000) + ts.tv_nsec as u64
}

fn thread_switches() -> (i64, i64) {
    let mut r: Rusage = unsafe { std::mem::zeroed() };
    let rc = unsafe { getrusage(RUSAGE_THREAD, &mut r) };
    if rc != 0 {
        return (0, 0);
    }
    (r.ru_nvcsw, r.ru_nivcsw)
}

thread_local! {
    static PERF: Cell<[i32; 3]> = const { Cell::new([-1, -1, -1]) };
    static PERF_DEAD: Cell<bool> = const { Cell::new(false) };
}

fn perf_open(type_: u32, config: u64) -> i32 {
    // perf_event_attr prefix. size=128, rest zero. exclude_kernel|exclude_hv.
    let mut raw = [0u8; 128];
    raw[0..4].copy_from_slice(&type_.to_ne_bytes());
    raw[4..8].copy_from_slice(&128u32.to_ne_bytes());
    raw[8..16].copy_from_slice(&config.to_ne_bytes());
    let flags: u64 = (1 << 5) | (1 << 6);
    raw[40..48].copy_from_slice(&flags.to_ne_bytes());
    let fd = unsafe { syscall(298, raw.as_ptr(), 0i64, -1i64, -1i64, 0u64) };
    fd as i32
}

fn perf_read() -> (bool, u64, u64, u64) {
    if PERF_DEAD.with(|d| d.get()) {
        return (false, 0, 0, 0);
    }
    let fds = PERF.with(|c| {
        let mut fds = c.get();
        if fds[0] < 0 {
            fds[0] = perf_open(0, 1);
            fds[1] = perf_open(0, 3);
            // LLC load miss: id=2, op=read=0, result=miss=1.
            fds[2] = perf_open(3, 2 | (1 << 16));
            c.set(fds);
            if fds[0] < 0 && fds[1] < 0 && fds[2] < 0 {
                PERF_DEAD.with(|d| d.set(true));
            }
        }
        fds
    });
    if fds[0] < 0 && fds[1] < 0 && fds[2] < 0 {
        return (false, 0, 0, 0);
    }
    let one = |fd: i32| -> u64 {
        if fd < 0 {
            return 0;
        }
        let mut buf = [0u8; 8];
        let n = unsafe { read(fd, buf.as_mut_ptr(), 8) };
        if n != 8 {
            return 0;
        }
        u64::from_ne_bytes(buf)
    };
    (fds[0] >= 0, one(fds[0]), one(fds[1]), one(fds[2]))
}

thread_local! {
    static ALLOC_N: Cell<u64> = const { Cell::new(0) };
    static ALLOC_B: Cell<u64> = const { Cell::new(0) };
}

fn alloc_now() -> (u64, u64) {
    let n = ALLOC_N.try_with(|c| c.get()).unwrap_or(0);
    let b = ALLOC_B.try_with(|c| c.get()).unwrap_or(0);
    (n, b)
}

fn alloc_add(size: u64) {
    if !alloc_on() {
        return;
    }
    let _ = ALLOC_N.try_with(|c| c.set(c.get().saturating_add(1)));
    let _ = ALLOC_B.try_with(|c| c.set(c.get().saturating_add(size)));
}

/// Counting wrapper around the system allocator. Install only on the
/// measurement example, and only for an alloc-count run.
pub struct InflationAlloc;

unsafe impl std::alloc::GlobalAlloc for InflationAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { std::alloc::System.alloc(layout) };
        if !p.is_null() {
            alloc_add(layout.size() as u64);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { std::alloc::System.alloc_zeroed(layout) };
        if !p.is_null() {
            alloc_add(layout.size() as u64);
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { std::alloc::System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            alloc_add(new_size as u64);
        }
        p
    }
}
