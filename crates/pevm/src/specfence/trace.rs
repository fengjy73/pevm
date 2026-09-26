//! In-block trace.
//!
//! Re-exec and full-replay counts always move. Per-read counters, durations,
//! and the profile clock move only when their flag is set. The profile flag
//! is `SPECFENCE_INFLATION`. It is off on the wall-clock path, so that path
//! does not call `Instant::now`.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

fn flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE")
    )
}

fn enabled_flag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| flag("SPECFENCE_INBLOCK_TRACE"))
}

fn profile_flag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| flag("SPECFENCE_INFLATION"))
}

fn diag_flag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| flag("SPECFENCE_ABORT_DIAG"))
}

/// Why `coordinate` let a read proceed or handed the worker back.
/// Recorded only when `SPECFENCE_ABORT_DIAG` is set, at the reader's read.
pub(crate) mod coord_reason {
    pub(crate) const NO_CHAIN: u8 = 1;
    pub(crate) const NO_LOWER: u8 = 2;
    pub(crate) const READY_PUB: u8 = 3;
    pub(crate) const READY_EXEC: u8 = 4;
    pub(crate) const SKIP_COST: u8 = 5;
    pub(crate) const WAITED_PUB: u8 = 6;
    pub(crate) const WAITED_EXEC: u8 = 7;
    pub(crate) const BLOCK: u8 = 8;

    pub(crate) const fn name(reason: u8) -> &'static str {
        match reason {
            NO_CHAIN => "no_chain",
            NO_LOWER => "no_lower_writer",
            READY_PUB => "ready_published",
            READY_EXEC => "ready_executed",
            SKIP_COST => "skip_cost",
            WAITED_PUB => "waited_published",
            WAITED_EXEC => "waited_executed",
            BLOCK => "blocked",
            _ => "unknown",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CoordNote {
    pub armed: bool,
    pub nearest: u32,
    pub state: u8,
    pub reason: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct AbortNote {
    pub reader: u32,
    pub location: u64,
    pub origin_tx: u32,
    pub live_tx: u32,
    pub live_in_chain_now: bool,
    pub live_state_now: u8,
    pub armed_now: bool,
    pub read: Option<CoordNote>,
}

/// One interpreter return, kept for the profile dump.
///
/// `kind` is 1 only after that incarnation commits. The report keeps the
/// committed row and ignores the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfAttempt {
    /// Transaction index.
    pub tx: u32,
    /// Incarnation.
    pub inc: u32,
    /// 1 after commit, 0 while the attempt is only an interpreter return.
    pub kind: u8,
    /// Nanoseconds from interpreter entry to the write-set publish.
    pub total_ns: u64,
    /// Nanoseconds inside the interpreter, excluding publish. Zero when the
    /// profile flag is off.
    pub interp_ns: u64,
    /// Locations read.
    pub reads: Vec<u64>,
    /// Non-lazy locations written.
    pub writes: Vec<u64>,
    /// Lazy locations written. The ideal schedule drops these.
    pub lazy_writes: Vec<u64>,
}

/// One folded read whose final value did not match the prediction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaNote {
    /// Transaction that folded the prediction.
    pub reader: u32,
    /// Basic-account hash.
    pub location: u64,
    /// Writer the validator blamed. `u32::MAX` when that side is absent.
    pub writer: u32,
    /// 1 base changed, 2 non-lazy write in the span, 3 amount, 4 estimate, 5 sealed without the credit.
    pub reason: u8,
}

pub(crate) struct Trace {
    on: bool,
    profile: bool,
    pub(crate) exec_entries: AtomicUsize,
    pub(crate) reexec: AtomicUsize,
    pub(crate) full_replay: AtomicUsize,
    pub(crate) reads_after_arm: AtomicUsize,
    pub(crate) full_replay_after_arm: AtomicUsize,
    /// Folded credit did not match the final delta.
    pub(crate) delta_mismatch: AtomicUsize,
    /// Those mismatches that aborted the reader.
    pub(crate) delta_abort: AtomicUsize,
    pub(crate) learned_active: AtomicUsize,
    pub(crate) learned_peak: AtomicUsize,
    pub(crate) parked: AtomicUsize,
    pub(crate) woken: AtomicUsize,
    pub(crate) hops: AtomicUsize,
    pub(crate) hops_same_worker: AtomicUsize,
    pub(crate) hop_gap_max_ns: AtomicU64,
    pub(crate) pipelined_hops: AtomicUsize,
    hot_loc: AtomicU64,
    hot_hops: AtomicUsize,
    hot_same: AtomicUsize,
    hot_gap: AtomicU64,
    hot_exec: AtomicU64,
    hot_span: AtomicU64,
    delta_notes: std::sync::Mutex<Vec<DeltaNote>>,
    pub(crate) raw_edges: std::sync::Mutex<Vec<(u32, u32)>>,
    pub(crate) tx_ns: std::sync::Mutex<Vec<u64>>,
    attempts: std::sync::Mutex<Vec<SfAttempt>>,
    diag: bool,
    /// Last coordinate decision per `(reader, location)`.
    decisions:
        std::sync::Mutex<hashbrown::HashMap<(u32, u64), CoordNote, rustc_hash::FxBuildHasher>>,
    aborts: std::sync::Mutex<Vec<AbortNote>>,
}

impl Trace {
    pub(crate) fn new(n: usize) -> Self {
        let on = enabled_flag();
        Self {
            on,
            profile: profile_flag(),
            exec_entries: AtomicUsize::new(0),
            reexec: AtomicUsize::new(0),
            full_replay: AtomicUsize::new(0),
            reads_after_arm: AtomicUsize::new(0),
            full_replay_after_arm: AtomicUsize::new(0),
            delta_mismatch: AtomicUsize::new(0),
            delta_abort: AtomicUsize::new(0),
            learned_active: AtomicUsize::new(0),
            learned_peak: AtomicUsize::new(0),
            parked: AtomicUsize::new(0),
            woken: AtomicUsize::new(0),
            hops: AtomicUsize::new(0),
            hops_same_worker: AtomicUsize::new(0),
            hop_gap_max_ns: AtomicU64::new(0),
            pipelined_hops: AtomicUsize::new(0),
            hot_loc: AtomicU64::new(0),
            hot_hops: AtomicUsize::new(0),
            hot_same: AtomicUsize::new(0),
            hot_gap: AtomicU64::new(0),
            hot_exec: AtomicU64::new(0),
            hot_span: AtomicU64::new(0),
            delta_notes: std::sync::Mutex::new(Vec::new()),
            raw_edges: std::sync::Mutex::new(Vec::new()),
            tx_ns: std::sync::Mutex::new(if on { vec![0; n] } else { Vec::new() }),
            attempts: std::sync::Mutex::new(Vec::new()),
            diag: diag_flag(),
            decisions: std::sync::Mutex::new(hashbrown::HashMap::with_hasher(
                rustc_hash::FxBuildHasher,
            )),
            aborts: std::sync::Mutex::new(Vec::new()),
        }
    }

    #[inline]
    pub(crate) const fn diag(&self) -> bool {
        self.diag
    }

    pub(crate) fn note_coord(
        &self,
        tx: usize,
        location: u64,
        armed: bool,
        nearest: u32,
        state: u8,
        reason: u8,
    ) {
        if !self.diag {
            return;
        }
        self.decisions.lock().unwrap().insert(
            (tx as u32, location),
            CoordNote {
                armed,
                nearest,
                state,
                reason,
            },
        );
    }

    pub(crate) fn note_abort(&self, note: AbortNote) {
        if !self.diag {
            return;
        }
        let read = self
            .decisions
            .lock()
            .unwrap()
            .get(&(note.reader, note.location))
            .copied();
        let mut note = note;
        note.read = read;
        self.aborts.lock().unwrap().push(note);
    }

    pub(crate) fn dump_aborts(&self) {
        if !self.diag {
            return;
        }
        let aborts = self.aborts.lock().unwrap();
        let mut hist: [usize; 9] = [0; 9];
        let mut armed_at_read = 0usize;
        let mut nearest_is_live = 0usize;
        let mut live_missing_at_read = 0usize;
        let mut no_decision = 0usize;
        for a in aborts.iter() {
            let Some(read) = a.read else {
                no_decision += 1;
                eprintln!(
                    "ABORT reader={} loc={:#x} origin={} live={} live_in_chain_now={} live_state_now={} armed_now={} read=NONE",
                    a.reader,
                    a.location,
                    a.origin_tx,
                    a.live_tx,
                    a.live_in_chain_now,
                    a.live_state_now,
                    a.armed_now,
                );
                continue;
            };
            if read.armed {
                armed_at_read += 1;
            }
            if read.reason < hist.len() as u8 {
                hist[read.reason as usize] += 1;
            }
            if a.live_tx != u32::MAX && read.nearest == a.live_tx {
                nearest_is_live += 1;
            } else if a.live_tx != u32::MAX {
                live_missing_at_read += 1;
            }
            eprintln!(
                "ABORT reader={} loc={:#x} origin={} live={} live_in_chain_now={} live_state_now={} armed_now={} read_armed={} read_nearest={} read_state={} reason={}",
                a.reader,
                a.location,
                a.origin_tx,
                a.live_tx,
                a.live_in_chain_now,
                a.live_state_now,
                a.armed_now,
                read.armed,
                read.nearest,
                read.state,
                coord_reason::name(read.reason),
            );
        }
        eprintln!(
            "ABORT_SUM n={} armed_at_read={} nearest_is_live={} live_not_nearest={} no_decision={} reasons={}",
            aborts.len(),
            armed_at_read,
            nearest_is_live,
            live_missing_at_read,
            no_decision,
            (1..hist.len())
                .filter(|&i| hist[i] > 0)
                .map(|i| format!("{}={}", coord_reason::name(i as u8), hist[i]))
                .collect::<Vec<_>>()
                .join(",")
        );
    }

    #[inline]
    pub(crate) const fn enabled(&self) -> bool {
        self.on
    }

    /// True when a duration or a profile attempt should be recorded.
    #[inline]
    pub(crate) const fn timing(&self) -> bool {
        self.on || self.profile
    }

    #[inline]
    pub(crate) const fn profile(&self) -> bool {
        self.profile
    }

    #[inline]
    pub(crate) fn note_hop(&self, same_worker: bool, gap_ns: u64, pipelined: bool) {
        self.hops.fetch_add(1, Ordering::Relaxed);
        if same_worker {
            self.hops_same_worker.fetch_add(1, Ordering::Relaxed);
        }
        if pipelined {
            self.pipelined_hops.fetch_add(1, Ordering::Relaxed);
        }
        let _ = self
            .hop_gap_max_ns
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
                (gap_ns > cur).then_some(gap_ns)
            });
    }

    pub(crate) fn note_delta(
        &self,
        reader: usize,
        location: u64,
        writer: Option<usize>,
        reason: u8,
    ) {
        if self.delta_notes.lock().unwrap().len() >= 8 {
            return;
        }
        self.delta_notes.lock().unwrap().push(DeltaNote {
            reader: reader as u32,
            location,
            writer: writer.map(|tx| tx as u32).unwrap_or(u32::MAX),
            reason,
        });
    }

    pub(crate) fn note_hot(
        &self,
        location: u64,
        hops: usize,
        same: usize,
        gap_ns: u64,
        exec_ns: u64,
        span_ns: u64,
    ) {
        self.hot_loc.store(location, Ordering::Relaxed);
        self.hot_hops.store(hops, Ordering::Relaxed);
        self.hot_same.store(same, Ordering::Relaxed);
        self.hot_gap.store(gap_ns, Ordering::Relaxed);
        self.hot_exec.store(exec_ns, Ordering::Relaxed);
        self.hot_span.store(span_ns, Ordering::Relaxed);
    }

    pub(crate) fn note_sched(&self, active: usize, peak: usize, parked: usize, woken: usize) {
        self.learned_active.store(active, Ordering::Relaxed);
        self.learned_peak.store(peak, Ordering::Relaxed);
        self.parked.store(parked, Ordering::Relaxed);
        self.woken.store(woken, Ordering::Relaxed);
    }

    pub(crate) fn note_exec(&self, incarnation: usize) {
        self.exec_entries.fetch_add(1, Ordering::Relaxed);
        if incarnation > 0 {
            self.reexec.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn note_read_after_arm(&self) {
        if self.on {
            self.reads_after_arm.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn note_full_replay(&self, armed: bool) {
        self.full_replay.fetch_add(1, Ordering::Relaxed);
        if self.on && armed {
            self.full_replay_after_arm.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn note_edge(&self, writer: usize, reader: usize) {
        if self.on && writer < reader {
            self.raw_edges
                .lock()
                .unwrap()
                .push((writer as u32, reader as u32));
        }
    }

    pub(crate) fn note_tx_ns(&self, tx: usize, ns: u64) {
        if !self.on {
            return;
        }
        let mut v = self.tx_ns.lock().unwrap();
        if let Some(slot) = v.get_mut(tx) {
            *slot = ns;
        }
    }

    pub(crate) fn note_attempt(
        &self,
        tx: usize,
        inc: usize,
        total_ns: u64,
        interp_ns: u64,
        reads: Vec<u64>,
        writes: Vec<u64>,
        lazy_writes: Vec<u64>,
    ) {
        if !self.profile {
            return;
        }
        self.attempts.lock().unwrap().push(SfAttempt {
            tx: tx as u32,
            inc: inc as u32,
            kind: 0,
            total_ns,
            interp_ns,
            reads,
            writes,
            lazy_writes,
        });
    }

    pub(crate) fn note_committed(&self, tx: usize, inc: usize) {
        if !self.profile {
            return;
        }
        let tx = tx as u32;
        let inc = inc as u32;
        let mut attempts = self.attempts.lock().unwrap();
        for attempt in attempts.iter_mut().rev() {
            if attempt.tx == tx && attempt.inc == inc {
                attempt.kind = 1;
                return;
            }
        }
    }
}

/// Counters for one `SpecFence` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfTrace {
    /// Extra executions whose incarnation was already above zero.
    pub reexec: usize,
    /// Validation failures that restarted the transaction from the beginning.
    pub full_replay: usize,
    /// Reads of a location after that location was armed.
    pub reads_after_arm: usize,
    /// Full replays whose failing location was already armed.
    pub full_replay_after_arm: usize,
    /// Longest in-block writer chain at the end of the block.
    pub chain_len: usize,
    /// Locations that entered the chain directory.
    pub armed: usize,
    /// Total interpreter entries, including the first execution of each tx.
    pub exec_entries: usize,
    /// Class key used for this run.
    pub class_key: String,
    /// Per-tx duration of the last successful execution, when tracing is on.
    pub tx_ns: Vec<u64>,
    /// RAW edges `(writer, reader)` observed from consumed origins.
    pub raw_edges: Vec<(u32, u32)>,
    /// Beneficiary basic-account hash. The ideal schedule drops this location.
    pub beneficiary: u64,
    /// Folded credits whose final amount differed from the prediction.
    pub delta_mismatch: usize,
    /// Reader aborts caused by those mismatches.
    pub delta_abort: usize,
    /// Active workers at the end of the block.
    pub learned_active: usize,
    /// Highest active-set size during the block.
    pub learned_peak: usize,
    /// Times a worker parked on a condvar.
    pub parked: usize,
    /// Times a parked worker was woken.
    pub woken: usize,
    /// RMW hops with a previous writer on the same location.
    pub hops: usize,
    /// Those hops whose previous writer ran on this worker.
    pub hops_same_worker: usize,
    /// Longest gap from the previous RMW end to this RMW start.
    pub hop_gap_max_ns: u64,
    /// Hops that started before the previous RMW finished.
    pub pipelined_hops: usize,
    /// Location with the most RMW hops.
    pub hot_location: u64,
    /// Hops on that location.
    pub hot_hops: usize,
    /// Those hops whose previous writer was this worker.
    pub hot_same: usize,
    /// Longest hop gap on that location.
    pub hot_gap_max_ns: u64,
    /// Sum of RMW execution on that location.
    pub hot_exec_ns: u64,
    /// First RMW start to last RMW end on that location.
    pub hot_span_ns: u64,
    /// Active-set size at each control decision, in order.
    pub active_samples: Vec<u16>,
    /// Location of the longest RMW gap, which may not be the hottest chain.
    pub gap_location: u64,
    /// Transaction that started after that gap.
    pub gap_tx: u32,
    /// Worker that finished the previous hop.
    pub gap_prev_worker: u32,
    /// Why `gap_tx` waited. `0` means it was runnable and had not been scheduled.
    pub gap_reason: u8,
    /// Up to eight folded mismatches, with the reader, location, and reason.
    pub delta_notes: Vec<DeltaNote>,
    /// Profile attempts. Empty unless `SPECFENCE_INFLATION` is set.
    pub attempts: Vec<SfAttempt>,
}

impl Trace {
    pub(crate) fn snapshot(
        &self,
        chain_len: usize,
        armed: usize,
        class_key: &str,
        beneficiary: u64,
    ) -> SfTrace {
        let (tx_ns, raw_edges) = if self.on {
            (
                self.tx_ns.lock().unwrap().clone(),
                self.raw_edges.lock().unwrap().clone(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let attempts = if self.profile {
            self.attempts.lock().unwrap().clone()
        } else {
            Vec::new()
        };
        SfTrace {
            reexec: self.reexec.load(Ordering::Relaxed),
            full_replay: self.full_replay.load(Ordering::Relaxed),
            reads_after_arm: self.reads_after_arm.load(Ordering::Relaxed),
            full_replay_after_arm: self.full_replay_after_arm.load(Ordering::Relaxed),
            chain_len,
            armed,
            exec_entries: self.exec_entries.load(Ordering::Relaxed),
            class_key: class_key.to_string(),
            tx_ns,
            raw_edges,
            beneficiary,
            delta_mismatch: self.delta_mismatch.load(Ordering::Relaxed),
            delta_abort: self.delta_abort.load(Ordering::Relaxed),
            learned_active: self.learned_active.load(Ordering::Relaxed),
            learned_peak: self.learned_peak.load(Ordering::Relaxed),
            parked: self.parked.load(Ordering::Relaxed),
            woken: self.woken.load(Ordering::Relaxed),
            hops: self.hops.load(Ordering::Relaxed),
            hops_same_worker: self.hops_same_worker.load(Ordering::Relaxed),
            hop_gap_max_ns: self.hop_gap_max_ns.load(Ordering::Relaxed),
            pipelined_hops: self.pipelined_hops.load(Ordering::Relaxed),
            hot_location: self.hot_loc.load(Ordering::Relaxed),
            hot_hops: self.hot_hops.load(Ordering::Relaxed),
            hot_same: self.hot_same.load(Ordering::Relaxed),
            hot_gap_max_ns: self.hot_gap.load(Ordering::Relaxed),
            hot_exec_ns: self.hot_exec.load(Ordering::Relaxed),
            hot_span_ns: self.hot_span.load(Ordering::Relaxed),
            active_samples: Vec::new(),
            gap_location: 0,
            gap_tx: 0,
            gap_prev_worker: 0,
            gap_reason: 0,
            delta_notes: self.delta_notes.lock().unwrap().clone(),
            attempts,
        }
    }
}
