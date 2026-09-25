//! In-block trace. Counters move only when `SPECFENCE_INBLOCK_TRACE` is set,
//! so the execution hot path does not touch them otherwise.

use std::sync::atomic::{AtomicUsize, Ordering};

fn enabled_flag() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("SPECFENCE_INBLOCK_TRACE").ok().as_deref(),
            Some("1" | "true" | "TRUE")
        )
    })
}

pub(crate) struct Trace {
    on: bool,
    pub(crate) exec_entries: AtomicUsize,
    pub(crate) reexec: AtomicUsize,
    pub(crate) full_replay: AtomicUsize,
    pub(crate) reads_after_arm: AtomicUsize,
    pub(crate) full_replay_after_arm: AtomicUsize,
    pub(crate) raw_edges: std::sync::Mutex<Vec<(u32, u32)>>,
    pub(crate) tx_ns: std::sync::Mutex<Vec<u64>>,
}

impl Trace {
    pub(crate) fn new(n: usize) -> Self {
        let on = enabled_flag();
        Self {
            on,
            exec_entries: AtomicUsize::new(0),
            reexec: AtomicUsize::new(0),
            full_replay: AtomicUsize::new(0),
            reads_after_arm: AtomicUsize::new(0),
            full_replay_after_arm: AtomicUsize::new(0),
            raw_edges: std::sync::Mutex::new(Vec::new()),
            tx_ns: std::sync::Mutex::new(vec![0; n]),
        }
    }

    #[inline]
    pub(crate) const fn enabled(&self) -> bool {
        self.on
    }

    #[inline]
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
}

impl Trace {
    pub(crate) fn snapshot(&self, chain_len: usize, armed: usize, class_key: &str) -> SfTrace {
        let (tx_ns, raw_edges) = if self.on {
            (
                self.tx_ns.lock().unwrap().clone(),
                self.raw_edges.lock().unwrap().clone(),
            )
        } else {
            (Vec::new(), Vec::new())
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
        }
    }
}
