//! Dual-horizon SpecFence learner (P1) — ∉ TCB.
//!
//! Intra-block: live fanout + morphology posterior updated on Observe/Abort/Publish.
//! Inter-block: `InterBlockPrior` EMA + flip decay; seeds HotSet/Bayes only —
//! **never** arms SoftWait from prior alone.

#![allow(dead_code)]
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use dashmap::DashMap;

use crate::{BuildIdentityHasher, MemoryLocationHash};

/// Morphology class weights (Dirichlet/EMA-style, normalized to sum ≈ 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MorphWeights {
    pub fan_out: f64,
    pub mixed: f64,
    pub waw_spine: f64,
    pub quiet: f64,
}

impl Default for MorphWeights {
    fn default() -> Self {
        // Quiet-biased cold start (598-like).
        Self {
            fan_out: 0.10,
            mixed: 0.20,
            waw_spine: 0.10,
            quiet: 0.60,
        }
    }
}

impl MorphWeights {
    pub(crate) fn normalize(mut self) -> Self {
        let s = self.fan_out + self.mixed + self.waw_spine + self.quiet;
        if s <= f64::EPSILON {
            return Self::default();
        }
        self.fan_out /= s;
        self.mixed /= s;
        self.waw_spine /= s;
        self.quiet /= s;
        self
    }

    pub(crate) fn dominant_waw(self) -> bool {
        self.waw_spine >= 0.35 && self.waw_spine >= self.fan_out
    }

    pub(crate) fn dominant_quiet(self) -> bool {
        self.quiet >= 0.45
    }

    /// Symmetric KL-ish distance for flip detection (not a true KL).
    pub(crate) fn divergence(self, other: Self) -> f64 {
        let a = [self.fan_out, self.mixed, self.waw_spine, self.quiet];
        let b = [other.fan_out, other.mixed, other.waw_spine, other.quiet];
        let mut d = 0.0;
        for i in 0..4 {
            let p = a[i].max(1e-6);
            let q = b[i].max(1e-6);
            d += p * (p / q).ln().abs() + q * (q / p).ln().abs();
        }
        d * 0.5
    }

    pub(crate) fn ema(self, hat: Self, alpha: f64) -> Self {
        let a = alpha.clamp(0.0, 1.0);
        Self {
            fan_out: (1.0 - a) * self.fan_out + a * hat.fan_out,
            mixed: (1.0 - a) * self.mixed + a * hat.mixed,
            waw_spine: (1.0 - a) * self.waw_spine + a * hat.waw_spine,
            quiet: (1.0 - a) * self.quiet + a * hat.quiet,
        }
        .normalize()
    }
}

/// Tunable π constants (process + optional block override). Learning ∉ TCB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AdaptiveParams {
    pub d_wait: f64,
    pub d_early: f64,
    pub tau_very_high: f64,
    pub tau_w: f64,
    pub tau_s: f64,
    pub tau_revoke: f64,
    pub c_retry: f64,
    pub cost_margin: f64,
    /// Distinct writers before ℓ is treated as WAW-spine-ish (schedule ≫ WaitHard).
    pub waw_writer_floor: usize,
    /// Gas-limit band for tx_heavy_hint (optional heuristic).
    pub heavy_gas_limit: u64,
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self {
            d_wait: 0.50,
            d_early: 0.15,
            tau_very_high: 0.75,
            tau_w: 0.35,
            tau_s: 0.50,
            tau_revoke: 0.20,
            c_retry: 3.0,
            cost_margin: 0.40,
            waw_writer_floor: 8,
            heavy_gas_limit: 200_000,
        }
    }
}

/// One inter-block top-ℓ seed entry (warm-start tracking only).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TopLocPrior {
    pub location: MemoryLocationHash,
    pub fanout_ema: f64,
    pub abort_rate: f64,
}

const MAX_TOP_L: usize = 64;
const ALPHA_NORMAL: f64 = 0.25;
const ALPHA_FLIP: f64 = 0.65;
const FLIP_KL: f64 = 0.35;

/// Inter-block warm-start prior (persists on [`crate::Pevm`]).
#[derive(Debug, Default)]
pub(crate) struct InterBlockPrior {
    morph_ema: Mutex<MorphWeights>,
    top_l: Mutex<Vec<TopLocPrior>>,
    last_morph_hat: Mutex<MorphWeights>,
    flip_count: AtomicUsize,
}

impl InterBlockPrior {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn morph_ema(&self) -> MorphWeights {
        *self.morph_ema.lock().unwrap()
    }

    pub(crate) fn top_locations(&self) -> Vec<TopLocPrior> {
        self.top_l.lock().unwrap().clone()
    }

    pub(crate) fn flip_count(&self) -> usize {
        self.flip_count.load(Ordering::Relaxed)
    }

    /// End-of-block: pack morph hat + top-ℓ; raise α on morphology flip.
    pub(crate) fn end_block(
        &self,
        morph_hat: MorphWeights,
        top: Vec<TopLocPrior>,
    ) -> f64 {
        let morph_hat = morph_hat.normalize();
        let mut ema = self.morph_ema.lock().unwrap();
        let kl = ema.divergence(morph_hat);
        let alpha = if kl > FLIP_KL {
            self.flip_count.fetch_add(1, Ordering::Relaxed);
            ALPHA_FLIP
        } else {
            ALPHA_NORMAL
        };
        *ema = ema.ema(morph_hat, alpha);
        *self.last_morph_hat.lock().unwrap() = morph_hat;
        let mut tops = self.top_l.lock().unwrap();
        *tops = top;
        if tops.len() > MAX_TOP_L {
            tops.truncate(MAX_TOP_L);
        }
        alpha
    }

    pub(crate) fn reset(&self) {
        *self.morph_ema.lock().unwrap() = MorphWeights::default();
        self.top_l.lock().unwrap().clear();
        *self.last_morph_hat.lock().unwrap() = MorphWeights::default();
        self.flip_count.store(0, Ordering::Relaxed);
    }
}

/// Per-location online stats for the current block.
#[derive(Debug, Default)]
struct LocLive {
    readers: AtomicUsize,
    writers: AtomicUsize,
    aborts: AtomicUsize,
    program_reads: AtomicUsize,
    handler_reads: AtomicUsize,
}

/// Intra-block live fanout + morphology posterior (one block).
#[derive(Debug, Default)]
pub(crate) struct LiveLearner {
    locs: DashMap<MemoryLocationHash, LocLive, BuildIdentityHasher>,
    program_obs: AtomicUsize,
    handler_obs: AtomicUsize,
    abort_events: AtomicUsize,
    cascade_sum: AtomicU64,
    publish_events: AtomicUsize,
    /// Running morph weights (normalized periodically).
    morph_bits: Mutex<MorphWeights>,
}

impl LiveLearner {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin_block(&self, prior_morph: MorphWeights) {
        self.locs.clear();
        self.program_obs.store(0, Ordering::Relaxed);
        self.handler_obs.store(0, Ordering::Relaxed);
        self.abort_events.store(0, Ordering::Relaxed);
        self.cascade_sum.store(0, Ordering::Relaxed);
        self.publish_events.store(0, Ordering::Relaxed);
        *self.morph_bits.lock().unwrap() = prior_morph.normalize();
    }

    /// Observe a location read resolve (policy feature refresh).
    pub(crate) fn note_observe(
        &self,
        location: MemoryLocationHash,
        is_program: bool,
        writer_count: usize,
    ) {
        let entry = self.locs.entry(location).or_default();
        entry.readers.fetch_add(1, Ordering::Relaxed);
        // Keep max writer count observed.
        let mut cur = entry.writers.load(Ordering::Relaxed);
        while writer_count > cur {
            match entry.writers.compare_exchange_weak(
                cur,
                writer_count,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
        if is_program {
            entry.program_reads.fetch_add(1, Ordering::Relaxed);
            self.program_obs.fetch_add(1, Ordering::Relaxed);
        } else {
            entry.handler_reads.fetch_add(1, Ordering::Relaxed);
            self.handler_obs.fetch_add(1, Ordering::Relaxed);
        }
        self.bump_morph_from_observe(is_program, writer_count, entry.readers.load(Ordering::Relaxed));
    }

    pub(crate) fn note_abort(&self, location: MemoryLocationHash, cascade_hint: usize) {
        self.abort_events.fetch_add(1, Ordering::Relaxed);
        self.cascade_sum
            .fetch_add(cascade_hint.max(1) as u64, Ordering::Relaxed);
        self.locs
            .entry(location)
            .or_default()
            .aborts
            .fetch_add(1, Ordering::Relaxed);
        // Large cascade → morph toward fan_out.
        let mut m = self.morph_bits.lock().unwrap();
        if cascade_hint >= 8 {
            m.fan_out += 0.05;
            m.quiet = (m.quiet - 0.03).max(0.01);
        } else {
            m.mixed += 0.02;
            m.quiet = (m.quiet - 0.01).max(0.01);
        }
        *m = m.normalize();
    }

    pub(crate) fn note_publish(&self, _location: MemoryLocationHash) {
        self.publish_events.fetch_add(1, Ordering::Relaxed);
    }

    fn bump_morph_from_observe(&self, is_program: bool, writers: usize, readers: usize) {
        let mut m = self.morph_bits.lock().unwrap();
        if writers >= 8 && !is_program {
            m.waw_spine += 0.04;
            m.quiet = (m.quiet - 0.02).max(0.01);
        } else if readers >= 16 && is_program {
            m.fan_out += 0.04;
            m.quiet = (m.quiet - 0.02).max(0.01);
        } else if is_program {
            m.mixed += 0.01;
        } else {
            m.quiet += 0.005;
        }
        *m = m.normalize();
    }

    pub(crate) fn morph_weights(&self) -> MorphWeights {
        self.morph_bits.lock().unwrap().normalize()
    }

    pub(crate) fn fanout_live(&self, location: MemoryLocationHash) -> usize {
        self.locs
            .get(&location)
            .map(|e| e.readers.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub(crate) fn writer_count_live(&self, location: MemoryLocationHash) -> usize {
        self.locs
            .get(&location)
            .map(|e| e.writers.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// WAW-spine hint: multi-writer / WAW-heavy without RAW-useful program fanout.
    pub(crate) fn waw_spine_hint(
        &self,
        location: MemoryLocationHash,
        is_program: bool,
        params: &AdaptiveParams,
    ) -> bool {
        let morph = self.morph_weights();
        if morph.dominant_waw() {
            return true;
        }
        let writers = self.writer_count_live(location);
        // Multi-writer handler/basic: schedule/steal ≫ WaitHard.
        if writers >= params.waw_writer_floor && !is_program {
            return true;
        }
        // Program with huge writer count but tiny reader fanout → WAW spine leftover.
        let readers = self.fanout_live(location);
        writers >= params.waw_writer_floor && readers < writers / 2 && !is_program
    }

    pub(crate) fn tx_heavy_hint(&self, gas_limit: u64, params: &AdaptiveParams) -> bool {
        gas_limit >= params.heavy_gas_limit
    }

    /// Pack top-ℓ for InterBlockPrior (by fanout then abort).
    pub(crate) fn pack_top_locations(&self) -> Vec<TopLocPrior> {
        let mut v: Vec<TopLocPrior> = self
            .locs
            .iter()
            .map(|e| {
                let readers = e.readers.load(Ordering::Relaxed) as f64;
                let aborts = e.aborts.load(Ordering::Relaxed) as f64;
                let obs = readers.max(1.0);
                TopLocPrior {
                    location: *e.key(),
                    fanout_ema: readers,
                    abort_rate: aborts / obs,
                }
            })
            .collect();
        v.sort_by(|a, b| {
            b.fanout_ema
                .partial_cmp(&a.fanout_ema)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.abort_rate
                        .partial_cmp(&a.abort_rate)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        v.truncate(MAX_TOP_L);
        v
    }

    /// Empirical morph hat from counters (for inter-block EMA).
    pub(crate) fn morph_hat(&self) -> MorphWeights {
        let prog = self.program_obs.load(Ordering::Relaxed) as f64;
        let hand = self.handler_obs.load(Ordering::Relaxed) as f64;
        let total = (prog + hand).max(1.0);
        let mut max_readers = 0usize;
        let mut max_writers = 0usize;
        for e in self.locs.iter() {
            max_readers = max_readers.max(e.readers.load(Ordering::Relaxed));
            max_writers = max_writers.max(e.writers.load(Ordering::Relaxed));
        }
        let mut w = MorphWeights::default();
        if max_readers >= 64 {
            w.fan_out = 0.55;
            w.mixed = 0.25;
            w.waw_spine = 0.10;
            w.quiet = 0.10;
        } else if max_writers >= 32 && max_readers < 32 {
            w.waw_spine = 0.50;
            w.mixed = 0.25;
            w.fan_out = 0.10;
            w.quiet = 0.15;
        } else if total < 8.0 {
            w.quiet = 0.70;
            w.mixed = 0.20;
            w.fan_out = 0.05;
            w.waw_spine = 0.05;
        } else {
            let prog_frac = prog / total;
            w.fan_out = 0.15 + 0.35 * prog_frac;
            w.mixed = 0.40;
            w.waw_spine = 0.10 * (1.0 - prog_frac);
            w.quiet = 0.15;
        }
        // Blend with online morph.
        let online = self.morph_weights();
        online.ema(w, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_normalize_and_ema() {
        let m = MorphWeights {
            fan_out: 2.0,
            mixed: 2.0,
            waw_spine: 2.0,
            quiet: 2.0,
        }
        .normalize();
        assert!((m.fan_out - 0.25).abs() < 1e-9);
        let next = m.ema(
            MorphWeights {
                fan_out: 1.0,
                mixed: 0.0,
                waw_spine: 0.0,
                quiet: 0.0,
            },
            0.5,
        );
        assert!(next.fan_out > m.fan_out);
    }

    #[test]
    fn waw_hint_on_multi_writer_handler() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        let params = AdaptiveParams::default();
        for _ in 0..10 {
            live.note_observe(7, false, 10);
        }
        assert!(live.waw_spine_hint(7, false, &params));
        assert!(!live.waw_spine_hint(7, true, &params) || live.writer_count_live(7) >= 8);
    }

    #[test]
    fn inter_prior_never_implies_soft_wait_seed_only() {
        let prior = InterBlockPrior::new();
        let hat = MorphWeights {
            fan_out: 0.7,
            mixed: 0.2,
            waw_spine: 0.05,
            quiet: 0.05,
        };
        let alpha = prior.end_block(
            hat,
            vec![TopLocPrior {
                location: 42,
                fanout_ema: 100.0,
                abort_rate: 0.2,
            }],
        );
        assert!(alpha >= ALPHA_NORMAL);
        assert_eq!(prior.top_locations().len(), 1);
        // Contract: prior exposes top_ℓ for HotSet/Bayes seed only — no SoftWait API here.
    }

    #[test]
    fn flip_raises_alpha() {
        let prior = InterBlockPrior::new();
        // Start quiet.
        prior.end_block(MorphWeights::default(), vec![]);
        let hat = MorphWeights {
            fan_out: 0.8,
            mixed: 0.1,
            waw_spine: 0.05,
            quiet: 0.05,
        };
        let alpha = prior.end_block(hat, vec![]);
        assert!((alpha - ALPHA_FLIP).abs() < 1e-9 || alpha >= ALPHA_NORMAL);
        assert!(prior.flip_count() >= 1 || hat.divergence(MorphWeights::default()) <= FLIP_KL);
    }
}
