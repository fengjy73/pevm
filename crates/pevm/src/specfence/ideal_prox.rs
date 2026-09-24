//! Soft=0 IdealProximityDiff. Measurement only.
//!
//! Off unless `SPECFENCE_IDEAL_PROXIMITY_DIFF=1` or `SPECFENCE_IDEAL_PROX_DIFF=1`.
//! Records the first `execute` enter and the successful finish against the
//! Detect antichain clock: a clean Indep has `ideal_ready = 0`. No Admit,
//! Avoid, Detect, or WaitOnce decision reads this log.

use std::fmt;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};

/// Enter was within ε of Ideal ready.
pub const BLOCKER_NONE: u8 = 0;
/// Still in an AdmitShard until this pop (supply lag).
pub const BLOCKER_ADMIT: u8 = 1;
/// Ordered spine hop.
pub const BLOCKER_SPINE: u8 = 2;
/// Detect predecessor edge (true tip / WaitOnce shape).
pub const BLOCKER_WAITONCE: u8 = 3;
/// Kept Detect consult. Not inferred on the schedule path.
pub const BLOCKER_DETECT: u8 = 4;
/// Conflicted retry shape. Label only; Avoid is not armed here.
pub const BLOCKER_AVOID: u8 = 5;
/// Validate queue. Not inferred on the enter path.
pub const BLOCKER_VALIDATE: u8 = 6;
/// Runnable width above twice the worker count. Secondary tag.
pub const BLOCKER_CORE: u8 = 7;
/// `finish_execution` tail. Not an enter blocker.
pub const BLOCKER_FINISH: u8 = 8;
/// Unclassified.
pub const BLOCKER_OTHER: u8 = 9;

pub const ROLE_OTHER: u8 = 0;
pub const ROLE_INDEP: u8 = 1;
pub const ROLE_SPINE: u8 = 2;
pub const ROLE_CONFLICT: u8 = 3;

/// Design ε. `|enter − ideal_ready|` inside this is `NoneIdealAligned`.
pub const ALIGN_EPS_MS: f64 = 0.05;

const BLOCKER_N: usize = 10;
const LAG_BINS: usize = 7;

pub fn blocker_name(b: u8) -> &'static str {
    match b {
        BLOCKER_NONE => "NoneIdealAligned",
        BLOCKER_ADMIT => "Admit",
        BLOCKER_SPINE => "Spine",
        BLOCKER_WAITONCE => "WaitOnce",
        BLOCKER_DETECT => "Detect",
        BLOCKER_AVOID => "Avoid",
        BLOCKER_VALIDATE => "Validate",
        BLOCKER_CORE => "CoreContention",
        BLOCKER_FINISH => "FinishPublish",
        _ => "Other",
    }
}

pub fn role_name(r: u8) -> &'static str {
    match r {
        ROLE_INDEP => "IndepClean",
        ROLE_SPINE => "SpineHop",
        ROLE_CONFLICT => "ConflictedNonSpine",
        _ => "Other",
    }
}

fn env_flag(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

/// Process-wide. The first read wins, before workers start.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        env_flag("SPECFENCE_IDEAL_PROXIMITY_DIFF") || env_flag("SPECFENCE_IDEAL_PROX_DIFF")
    })
}

/// Schedule-time primary class. Detect / Avoid / Validate / FinishPublish
/// are not guessed here.
pub(crate) fn classify_enter(
    ordered: bool,
    has_pred: bool,
    width: usize,
    cores: usize,
) -> (u8, u8, u8, u16) {
    let preds = u16::from(has_pred);
    let role = if ordered {
        ROLE_SPINE
    } else if has_pred {
        ROLE_CONFLICT
    } else {
        ROLE_INDEP
    };
    let secondary = if cores > 0 && width > cores.saturating_mul(2) {
        BLOCKER_CORE
    } else {
        BLOCKER_NONE
    };
    let blocker = if ordered {
        BLOCKER_SPINE
    } else if has_pred {
        BLOCKER_WAITONCE
    } else {
        BLOCKER_ADMIT
    };
    (blocker, secondary, role, preds)
}

fn lag_bin(delta_ms: f64) -> usize {
    if delta_ms <= 0.0 {
        0
    } else if delta_ms <= 0.2 {
        1
    } else if delta_ms <= 0.5 {
        2
    } else if delta_ms <= 1.19 {
        3
    } else if delta_ms <= 2.089 {
        4
    } else if delta_ms <= 5.0 {
        5
    } else {
        6
    }
}

fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let n = n as f64;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    let mut sxy = 0.0;
    for (x, y) in xs.iter().zip(ys.iter()) {
        sx += x;
        sy += y;
        sxx += x * x;
        syy += y * y;
        sxy += x * y;
    }
    let num = n * sxy - sx * sy;
    let den = ((n * sxx - sx * sx) * (n * syy - sy * sy)).sqrt();
    if den == 0.0 { 0.0 } else { num / den }
}

/// One tx. `enter_ns == 0` means execute never started. Times are nanoseconds
/// from the parallel-phase origin (same origin as `tx_first_start`).
#[derive(Clone, Debug)]
pub struct IdealProxTx {
    pub tx: u32,
    pub enter_ns: u64,
    pub finish_ns: u64,
    pub attempts: u32,
    pub blocker: u8,
    pub secondary: u8,
    pub role: u8,
    pub preds: u16,
    pub wave: u16,
}

#[derive(Clone, Debug, Default)]
pub struct IdealProxSnap {
    pub enabled: bool,
    pub txs: Vec<IdealProxTx>,
}

/// IndepClean wave-lag aggregate. `ideal_ready = 0` for that role.
/// `ideal_finish` is not invented: the offline per-tx work column is absent.
#[derive(Clone, Debug)]
pub struct IdealProxDiff {
    pub indep_n: usize,
    pub entered_indep: usize,
    pub not_entered_indep: usize,
    pub aligned_n: usize,
    /// Bins: `(-∞,0]`, `(0,0.2]`, `(0.2,0.5]`, `(0.5,1.19]`, `(1.19,2.089]`,
    /// `(2.089,5]`, `(5,+∞)` milliseconds. ε-aligned txs can still sit in `(0,0.2]`.
    pub wave_lag_bins: [usize; LAG_BINS],
    pub blocker_counts: [usize; BLOCKER_N],
    pub blocker_lag_sum_ms: [f64; BLOCKER_N],
    /// Fraction of IndepClean with `enter_ms <=` 0.2, 0.5, 1.19, 2.089.
    pub fill_at: [f64; 4],
    pub corr_tx_enter: f64,
    pub median_enter_ms: f64,
    pub enter_eq_first_start: bool,
    pub max_enter_minus_first_start_ns: u64,
}

impl Default for IdealProxDiff {
    fn default() -> Self {
        Self {
            indep_n: 0,
            entered_indep: 0,
            not_entered_indep: 0,
            aligned_n: 0,
            wave_lag_bins: [0; LAG_BINS],
            blocker_counts: [0; BLOCKER_N],
            blocker_lag_sum_ms: [0.0; BLOCKER_N],
            fill_at: [0.0; 4],
            corr_tx_enter: 0.0,
            median_enter_ms: 0.0,
            enter_eq_first_start: true,
            max_enter_minus_first_start_ns: 0,
        }
    }
}

pub fn diff_indep(snap: &IdealProxSnap, first_start: &[u64]) -> IdealProxDiff {
    let mut out = IdealProxDiff::default();
    if !snap.enabled {
        return out;
    }
    let mut enters = Vec::new();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut max_delta = 0u64;
    let mut eq = true;
    for row in &snap.txs {
        if row.role != ROLE_INDEP {
            continue;
        }
        out.indep_n += 1;
        let start = first_start.get(row.tx as usize).copied().unwrap_or(0);
        if row.enter_ns == 0 {
            out.not_entered_indep += 1;
            continue;
        }
        out.entered_indep += 1;
        let enter_ms = row.enter_ns as f64 / 1e6;
        enters.push(enter_ms);
        xs.push(row.tx as f64);
        ys.push(enter_ms);
        let delta_ns = row.enter_ns.abs_diff(start);
        max_delta = max_delta.max(delta_ns);
        if start != 0 && delta_ns > 5_000 {
            eq = false;
        }
        let bin = lag_bin(enter_ms);
        out.wave_lag_bins[bin] += 1;
        let class = if enter_ms.abs() <= ALIGN_EPS_MS {
            out.aligned_n += 1;
            BLOCKER_NONE
        } else {
            usize::from(row.blocker).min(BLOCKER_N - 1) as u8
        };
        let ci = usize::from(class).min(BLOCKER_N - 1);
        out.blocker_counts[ci] += 1;
        out.blocker_lag_sum_ms[ci] += enter_ms;
    }
    out.enter_eq_first_start = eq;
    out.max_enter_minus_first_start_ns = max_delta;
    if out.indep_n > 0 {
        let n = out.indep_n as f64;
        for (i, edge) in [0.2_f64, 0.5, 1.19, 2.089].into_iter().enumerate() {
            let hit = enters.iter().filter(|ms| **ms <= edge).count();
            out.fill_at[i] = hit as f64 / n;
        }
    }
    enters.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if !enters.is_empty() {
        out.median_enter_ms = enters[enters.len() / 2];
    }
    out.corr_tx_enter = pearson(&xs, &ys);
    out
}

/// Per-block clocks. Empty vectors when the flag is off (no per-tx atomics).
pub struct IdealProxLog {
    on: bool,
    enter_ns: Vec<AtomicU64>,
    finish_ns: Vec<AtomicU64>,
    attempts: Vec<AtomicU32>,
    blocker: Vec<AtomicU8>,
    secondary: Vec<AtomicU8>,
    role: Vec<AtomicU8>,
    preds: Vec<AtomicU16>,
    wave: Vec<AtomicU16>,
}

impl fmt::Debug for IdealProxLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdealProxLog")
            .field("on", &self.on)
            .field("n", &self.enter_ns.len())
            .finish()
    }
}

impl IdealProxLog {
    pub(crate) fn new(n: usize) -> Self {
        if !enabled() {
            return Self {
                on: false,
                enter_ns: Vec::new(),
                finish_ns: Vec::new(),
                attempts: Vec::new(),
                blocker: Vec::new(),
                secondary: Vec::new(),
                role: Vec::new(),
                preds: Vec::new(),
                wave: Vec::new(),
            };
        }
        Self {
            on: true,
            enter_ns: (0..n).map(|_| AtomicU64::new(0)).collect(),
            finish_ns: (0..n).map(|_| AtomicU64::new(0)).collect(),
            attempts: (0..n).map(|_| AtomicU32::new(0)).collect(),
            blocker: (0..n).map(|_| AtomicU8::new(0)).collect(),
            secondary: (0..n).map(|_| AtomicU8::new(0)).collect(),
            role: (0..n).map(|_| AtomicU8::new(0)).collect(),
            preds: (0..n).map(|_| AtomicU16::new(0)).collect(),
            wave: (0..n).map(|_| AtomicU16::new(0)).collect(),
        }
    }

    #[inline]
    pub(crate) fn enabled(&self) -> bool {
        self.on
    }

    #[inline]
    pub(crate) fn note_enter(
        &self,
        tx: usize,
        ns: u64,
        blocker: u8,
        secondary: u8,
        role: u8,
        preds: u16,
        wave: u16,
    ) {
        if !self.on || tx >= self.enter_ns.len() {
            return;
        }
        let ns = ns.max(1);
        if self.enter_ns[tx]
            .compare_exchange(0, ns, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.blocker[tx].store(blocker, Ordering::Relaxed);
            self.secondary[tx].store(secondary, Ordering::Relaxed);
            self.role[tx].store(role, Ordering::Relaxed);
            self.preds[tx].store(preds, Ordering::Relaxed);
            self.wave[tx].store(wave, Ordering::Relaxed);
        }
        self.attempts[tx].fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn note_finish(&self, tx: usize, ns: u64) {
        if !self.on || tx >= self.finish_ns.len() {
            return;
        }
        self.finish_ns[tx].store(ns.max(1), Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> IdealProxSnap {
        if !self.on {
            return IdealProxSnap::default();
        }
        let txs = (0..self.enter_ns.len())
            .map(|i| IdealProxTx {
                tx: i as u32,
                enter_ns: self.enter_ns[i].load(Ordering::Relaxed),
                finish_ns: self.finish_ns[i].load(Ordering::Relaxed),
                attempts: self.attempts[i].load(Ordering::Relaxed),
                blocker: self.blocker[i].load(Ordering::Relaxed),
                secondary: self.secondary[i].load(Ordering::Relaxed),
                role: self.role[i].load(Ordering::Relaxed),
                preds: self.preds[i].load(Ordering::Relaxed),
                wave: self.wave[i].load(Ordering::Relaxed),
            })
            .collect();
        IdealProxSnap { enabled: true, txs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_bins_match_the_design_edges() {
        assert_eq!(lag_bin(0.0), 0);
        assert_eq!(lag_bin(-0.01), 0);
        assert_eq!(lag_bin(0.05), 1);
        assert_eq!(lag_bin(0.2), 1);
        assert_eq!(lag_bin(0.5), 2);
        assert_eq!(lag_bin(1.19), 3);
        assert_eq!(lag_bin(2.089), 4);
        assert_eq!(lag_bin(5.0), 5);
        assert_eq!(lag_bin(5.1), 6);
    }

    #[test]
    fn clean_indep_within_eps_is_aligned() {
        let snap = IdealProxSnap {
            enabled: true,
            txs: vec![
                IdealProxTx {
                    tx: 0,
                    enter_ns: 10_000,
                    finish_ns: 20_000,
                    attempts: 1,
                    blocker: BLOCKER_ADMIT,
                    secondary: BLOCKER_NONE,
                    role: ROLE_INDEP,
                    preds: 0,
                    wave: 0,
                },
                IdealProxTx {
                    tx: 4,
                    enter_ns: 3_000_000,
                    finish_ns: 3_100_000,
                    attempts: 1,
                    blocker: BLOCKER_ADMIT,
                    secondary: BLOCKER_NONE,
                    role: ROLE_INDEP,
                    preds: 0,
                    wave: 0,
                },
            ],
        };
        let diff = diff_indep(&snap, &[10_000, 0, 0, 0, 3_000_000]);
        assert_eq!(diff.indep_n, 2);
        assert_eq!(diff.aligned_n, 1);
        assert_eq!(diff.blocker_counts[usize::from(BLOCKER_NONE)], 1);
        assert_eq!(diff.blocker_counts[usize::from(BLOCKER_ADMIT)], 1);
        assert!(diff.wave_lag_bins[5] >= 1, "3 ms sits in (2.089, 5]");
        assert!(diff.enter_eq_first_start);
    }
}
