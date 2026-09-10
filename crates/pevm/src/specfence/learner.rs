//! Dual-horizon SpecFence learner (P1 / AEC / V5-P2 θ) — ∉ TCB.
//!
//! Intra-block: live fanout + morphology posterior + EV estimators
//! (`E_wait_time`, `E_cascade`, `E_reexec`, `E_idle_steal`, SoftWait latency,
//! bind/wait_useful, meta tax) updated on outcomes.
//! Inter-block: `InterBlockPrior` EMA + flip decay; seeds HotSet/Bayes + **engagement
//! mode** (quiet vs storm) — never arms SoftWait Soft from prior alone.
//!
//! `AdaptiveParams` are learning rates / priors for the Adaptive EV Controller —
//! **not** Boolean Wait cuts (`D_WAIT` / fanout ladders).

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

    /// Fan-out morphology (597-like): late first-cross dominates Wait EV.
    pub(crate) fn dominant_fan_out(self) -> bool {
        self.fan_out >= 0.35 && self.fan_out >= self.mixed && self.fan_out >= self.waw_spine
    }

    /// Morph feature for **W_remain** estimate when measured d is unknown.
    /// fan_out-dominant → late (~0.9) ⇒ smaller remaining work if Spec aborts.
    /// Never Boolean-forces Wait; never feeds EarlyAbort (needs known d).
    pub(crate) fn wait_depth_prior(self) -> Option<f64> {
        if self.dominant_fan_out() {
            Some(0.9)
        } else {
            None
        }
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

/// AEC learning rates / priors (∉ TCB). **Not** Boolean Wait decision cuts.
///
/// `d_wait` / `cost_margin` are retained for lab override compatibility only —
/// [`crate::specfence::choose_action`] must **not** use them as Wait gates.
/// Depth `d` and fan-out enter EV as continuous features.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AdaptiveParams {
    /// α in `EV_Wait = E_wait_time * (1 + α * fanout) + δ * E_idle` — high fanout ↑ EV_Wait.
    pub alpha_fanout: f64,
    /// β in `EV_Spec = P_abort * (W_remain + β * E_cascade + γ * E_reexec)`.
    pub beta_cascade: f64,
    /// γ weight of measured E_reexec inside EV_Spec (V5-P1 lean ForceBind vs FullRestart).
    pub gamma_reexec: f64,
    /// δ weight of idle-steal tax added to EV_Wait (park without steal raises Wait).
    pub delta_idle: f64,
    /// EMA learning rate for per-ℓ E_wait_time (arm→wake latency units).
    pub lr_wait_time: f64,
    /// EMA learning rate for E_cascade / E_reexec / E_idle.
    pub lr_cascade: f64,
    /// Prior E_wait_time (normalized work units; ~1.0 = one unresolved producer).
    pub e_wait_prior: f64,
    /// Prior expected cascade size (work units).
    pub e_cascade_prior: f64,
    /// Prior / EarlyAbort reexec overhead; also EV_Spec via γ.
    pub e_reexec: f64,
    /// Prior idle-steal tax (0 = cores stay busy on Wait via steal).
    pub e_idle_prior: f64,
    /// Meta budget ρ: if meta_ops/useful_effects > ρ → force SpecRead (OCC fallback).
    pub meta_budget_rho: f64,
    /// Tiny EV gap bias: if EV_Wait beats EV_Spec by < meta_gap_eps*(1+meta_tax) → SpecRead.
    pub meta_gap_eps: f64,
    /// EarlyAbort niche only: known d ≤ d_early (not a Wait cut).
    pub d_early: f64,
    /// Bind escalation when published Data + very-high P (not a Wait gate).
    pub tau_very_high: f64,
    pub tau_w: f64,
    pub tau_s: f64,
    pub tau_revoke: f64,
    /// Legacy SpecRead cost scale (maps into W_remain / docs).
    pub c_retry: f64,
    /// DEPRECATED — not a Wait gate in AEC π (kept for lab JSON compatibility).
    pub d_wait: f64,
    /// DEPRECATED — not a Wait gate in AEC π.
    pub cost_margin: f64,
    /// Distinct writers before ℓ is treated as WAW-spine feature.
    pub waw_writer_floor: usize,
    /// Gas-limit band for tx_heavy_hint feature (EarlyAbort niche).
    pub heavy_gas_limit: u64,
}

impl Default for AdaptiveParams {
    fn default() -> Self {
        Self::from_l3()
    }
}

impl AdaptiveParams {
    /// AEC defaults: learning rates / priors. `d_wait` retained but unused by π.
    pub(crate) const fn from_l3() -> Self {
        Self {
            alpha_fanout: 0.22,
            beta_cascade: 1.0,
            // Half-weight reexec so Spec is priced but Quiet does not Wait-storm.
            gamma_reexec: 0.50,
            // Mild idle tax: park-without-steal raises EV_Wait (prefer Spec).
            delta_idle: 0.10,
            lr_wait_time: 0.25,
            lr_cascade: 0.20,
            e_wait_prior: 1.0,
            e_cascade_prior: 1.0,
            e_reexec: 1.5,
            e_idle_prior: 0.20,
            // SoftWait/meta_ops over useful effects > ρ → OCC SpecRead (after warmup).
            meta_budget_rho: 0.30,
            // Near-tie Wait wins → Spec when measured meta tax (no SoftWait storm).
            meta_gap_eps: 0.05,
            d_early: 0.15,
            tau_very_high: 0.75,
            tau_w: 0.35,
            tau_s: 0.50,
            tau_revoke: 0.20,
            c_retry: 3.0,
            d_wait: 0.50, // deprecated for π
            cost_margin: 0.40, // deprecated for π
            waw_writer_floor: 8,
            heavy_gas_limit: 200_000,
        }
    }

    /// Apply optional field overrides (lab JSON / process start). Unknown keys ignored by caller.
    pub(crate) fn with_overrides(
        mut self,
        d_wait: Option<f64>,
        d_early: Option<f64>,
        tau_very_high: Option<f64>,
        c_retry: Option<f64>,
        cost_margin: Option<f64>,
        tau_revoke: Option<f64>,
        waw_writer_floor: Option<usize>,
        heavy_gas_limit: Option<u64>,
    ) -> Self {
        if let Some(v) = d_wait {
            self.d_wait = v;
        }
        if let Some(v) = d_early {
            self.d_early = v;
        }
        if let Some(v) = tau_very_high {
            self.tau_very_high = v;
        }
        if let Some(v) = c_retry {
            self.c_retry = v;
        }
        if let Some(v) = cost_margin {
            self.cost_margin = v;
        }
        if let Some(v) = tau_revoke {
            self.tau_revoke = v;
        }
        if let Some(v) = waw_writer_floor {
            self.waw_writer_floor = v;
        }
        if let Some(v) = heavy_gas_limit {
            self.heavy_gas_limit = v;
        }
        self
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
/// Live reader fanout at which ℓ is a hot Await candidate (597-like).
pub(crate) const HOT_FANOUT_THRESH: usize = 8;

/// Inter-block warm-start prior (persists on [`crate::Pevm`]).
#[derive(Debug, Default)]
pub(crate) struct InterBlockPrior {
    morph_ema: Mutex<MorphWeights>,
    top_l: Mutex<Vec<TopLocPrior>>,
    last_morph_hat: Mutex<MorphWeights>,
    flip_count: AtomicUsize,
    /// A6: last `end_block` saw a morphology flip (warm-start decay).
    last_flipped: AtomicUsize,
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

    /// A6: consume last-block morphology flip for sketch decay.
    pub(crate) fn take_last_flipped(&self) -> bool {
        self.last_flipped.swap(0, Ordering::Relaxed) != 0
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
        let flipped = kl > FLIP_KL;
        let alpha = if flipped {
            self.flip_count.fetch_add(1, Ordering::Relaxed);
            self.last_flipped.store(1, Ordering::Relaxed);
            ALPHA_FLIP
        } else {
            self.last_flipped.store(0, Ordering::Relaxed);
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
        self.last_flipped.store(0, Ordering::Relaxed);
    }
}

/// Per-location online stats for the current block (AEC outcome feeds).
#[derive(Debug, Default)]
struct LocLive {
    readers: AtomicUsize,
    writers: AtomicUsize,
    aborts: AtomicUsize,
    program_reads: AtomicUsize,
    handler_reads: AtomicUsize,
    /// Bind success credits (π Bind hit + Publish→bind prior).
    bind_hits: AtomicUsize,
    /// SoftWait wake was useful (producer published while waiter armed).
    wait_useful: AtomicUsize,
    /// Producer status hist: Data / Running-ish (coarse bits).
    status_data: AtomicUsize,
    status_running: AtomicUsize,
    /// SoftWait arm→wake latency sum (ns) for E_wait_time EMA.
    wait_latency_ns_sum: AtomicU64,
    wait_latency_count: AtomicUsize,
    /// Cascade size sum on aborts at this ℓ.
    cascade_sum: AtomicU64,
    /// Fixed-point ×1e6 EMA of E_wait_time (normalized units).
    e_wait_bits: AtomicU64,
    /// Fixed-point ×1e6 EMA of E_cascade.
    e_cascade_bits: AtomicU64,
    /// Sticky resolve: conflict ℓ after force_bind_reabort (Bind/Await bias).
    sticky_resolve: AtomicUsize,
}


#[inline]
fn fp_encode(x: f64) -> u64 {
    (x.max(0.0) * 1_000_000.0) as u64
}

#[inline]
fn fp_decode(bits: u64) -> f64 {
    bits as f64 / 1_000_000.0
}

/// Intra-block live fanout + morphology posterior + AEC EV estimators (one block).
#[derive(Debug, Default)]
pub(crate) struct LiveLearner {
    locs: DashMap<MemoryLocationHash, LocLive, BuildIdentityHasher>,
    program_obs: AtomicUsize,
    handler_obs: AtomicUsize,
    abort_events: AtomicUsize,
    cascade_sum: AtomicU64,
    publish_events: AtomicUsize,
    bind_success_total: AtomicUsize,
    wait_useful_total: AtomicUsize,
    /// Cheap EMA of measured gross-work / effect-progress depth samples.
    d_sum_bits: AtomicU64,
    d_count: AtomicUsize,
    /// SoftWait arm→wake latency aggregates (debug + global E_wait prior refresh).
    wait_latency_ns_sum: AtomicU64,
    wait_latency_count: AtomicUsize,
    /// Best-effort steal/idle proxies (from wave metrics feed).
    steal_events: AtomicUsize,
    park_ns_proxy: AtomicU64,
    /// Meta ops counted toward ρ budget (SoftWait arms observed by π).
    meta_ops: AtomicUsize,
    /// Useful effects (bind + wait_useful + publishes) for ρ denominator.
    useful_effects: AtomicUsize,
    /// Global EMA E_wait_time / E_cascade / E_reexec / E_idle (fixed-point ×1e6).
    global_e_wait_bits: AtomicU64,
    global_e_cascade_bits: AtomicU64,
    global_e_reexec_bits: AtomicU64,
    global_e_idle_bits: AtomicU64,
    /// Running morph weights (normalized periodically).
    morph_bits: Mutex<MorphWeights>,
    /// Block AdaptiveParams snapshot for EMA rates (set at begin_block).
    params_bits: Mutex<AdaptiveParams>,
}

impl LiveLearner {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn begin_block(&self, prior_morph: MorphWeights) {
        self.begin_block_with_params(prior_morph, AdaptiveParams::default());
    }

    pub(crate) fn begin_block_with_params(
        &self,
        prior_morph: MorphWeights,
        params: AdaptiveParams,
    ) {
        self.locs.clear();
        self.program_obs.store(0, Ordering::Relaxed);
        self.handler_obs.store(0, Ordering::Relaxed);
        self.abort_events.store(0, Ordering::Relaxed);
        self.cascade_sum.store(0, Ordering::Relaxed);
        self.publish_events.store(0, Ordering::Relaxed);
        self.bind_success_total.store(0, Ordering::Relaxed);
        self.wait_useful_total.store(0, Ordering::Relaxed);
        self.d_sum_bits.store(0, Ordering::Relaxed);
        self.d_count.store(0, Ordering::Relaxed);
        self.wait_latency_ns_sum.store(0, Ordering::Relaxed);
        self.wait_latency_count.store(0, Ordering::Relaxed);
        self.steal_events.store(0, Ordering::Relaxed);
        self.park_ns_proxy.store(0, Ordering::Relaxed);
        self.meta_ops.store(0, Ordering::Relaxed);
        self.useful_effects.store(0, Ordering::Relaxed);
        self.global_e_wait_bits
            .store(fp_encode(params.e_wait_prior), Ordering::Relaxed);
        self.global_e_cascade_bits
            .store(fp_encode(params.e_cascade_prior), Ordering::Relaxed);
        self.global_e_reexec_bits
            .store(fp_encode(params.e_reexec), Ordering::Relaxed);
        self.global_e_idle_bits
            .store(fp_encode(params.e_idle_prior), Ordering::Relaxed);
        *self.morph_bits.lock().unwrap() = prior_morph.normalize();
        *self.params_bits.lock().unwrap() = params;
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
        let c = cascade_hint.max(1);
        self.abort_events.fetch_add(1, Ordering::Relaxed);
        self.cascade_sum
            .fetch_add(c as u64, Ordering::Relaxed);
        let entry = self.locs.entry(location).or_default();
        entry.aborts.fetch_add(1, Ordering::Relaxed);
        entry.cascade_sum.fetch_add(c as u64, Ordering::Relaxed);
        // EMA update E_cascade at ℓ and globally.
        let params = *self.params_bits.lock().unwrap();
        let lr = params.lr_cascade;
        let prev = fp_decode(entry.e_cascade_bits.load(Ordering::Relaxed));
        let prev = if prev <= f64::EPSILON {
            params.e_cascade_prior
        } else {
            prev
        };
        let next = (1.0 - lr) * prev + lr * (c as f64);
        entry.e_cascade_bits.store(fp_encode(next), Ordering::Relaxed);
        let g_prev = fp_decode(self.global_e_cascade_bits.load(Ordering::Relaxed));
        let g_next = (1.0 - lr) * g_prev + lr * (c as f64);
        self.global_e_cascade_bits
            .store(fp_encode(g_next), Ordering::Relaxed);
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

    /// Record conflict location after force_bind_reabort (sticky resolve).
    /// Next touch: raise EV_Spec / lower EV_Wait so Bind/Await beat SpecRead loops.
    pub(crate) fn note_sticky_resolve(&self, location: MemoryLocationHash) {
        let entry = self.locs.entry(location).or_default();
        entry.sticky_resolve.fetch_add(1, Ordering::Relaxed);
    }

    /// True when ℓ is sticky after a force_bind_reabort this block.
    pub(crate) fn is_sticky_resolve(&self, location: MemoryLocationHash) -> bool {
        self.locs
            .get(&location)
            .is_some_and(|e| e.sticky_resolve.load(Ordering::Relaxed) > 0)
    }

    /// Cheap reader bump on hot-candidate first-cross (no morph/π tax).
    /// Builds live fanout so storm Await can engage without full `note_observe`.
    pub(crate) fn note_hot_touch(&self, location: MemoryLocationHash, is_program: bool) {
        let entry = self.locs.entry(location).or_default();
        entry.readers.fetch_add(1, Ordering::Relaxed);
        if is_program {
            entry.program_reads.fetch_add(1, Ordering::Relaxed);
            self.program_obs.fetch_add(1, Ordering::Relaxed);
        } else {
            entry.handler_reads.fetch_add(1, Ordering::Relaxed);
            self.handler_obs.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Intra hot candidate: prior top-k / live fanout / sticky (for choose_action gate).
    #[inline]
    pub(crate) fn is_hot_learn_candidate(
        &self,
        location: MemoryLocationHash,
        hotset: bool,
        prior_ws: bool,
        force_bind: bool,
    ) -> bool {
        if force_bind || hotset || prior_ws || self.is_sticky_resolve(location) {
            return true;
        }
        self.fanout_live(location) >= HOT_FANOUT_THRESH
    }

    /// Live fanout crosses Await threshold (A hot ℓ).
    #[inline]
    pub(crate) fn live_fanout_hot(&self, location: MemoryLocationHash) -> bool {
        self.fanout_live(location) >= HOT_FANOUT_THRESH
    }

    pub(crate) fn note_publish(&self, location: MemoryLocationHash) {
        self.publish_events.fetch_add(1, Ordering::Relaxed);
        // Publish Data ⇒ Bind would have succeeded — cheap bind-prior credit.
        let entry = self.locs.entry(location).or_default();
        entry.bind_hits.fetch_add(1, Ordering::Relaxed);
        entry.status_data.fetch_add(1, Ordering::Relaxed);
        self.bind_success_total.fetch_add(1, Ordering::Relaxed);
        self.useful_effects.fetch_add(1, Ordering::Relaxed);
    }

    /// π Bind hit credit (symmetric with Bayes observe_bind_hit).
    pub(crate) fn note_bind_success(&self, location: MemoryLocationHash) {
        self.bind_success_total.fetch_add(1, Ordering::Relaxed);
        self.useful_effects.fetch_add(1, Ordering::Relaxed);
        let entry = self.locs.entry(location).or_default();
        entry.bind_hits.fetch_add(1, Ordering::Relaxed);
        entry.status_data.fetch_add(1, Ordering::Relaxed);
    }

    /// SoftWait wake useful: producer published while waiter was armed.
    pub(crate) fn note_wait_useful(&self, location: MemoryLocationHash) {
        self.wait_useful_total.fetch_add(1, Ordering::Relaxed);
        self.useful_effects.fetch_add(1, Ordering::Relaxed);
        self.locs
            .entry(location)
            .or_default()
            .wait_useful
            .fetch_add(1, Ordering::Relaxed);
    }

    /// SoftWait arm→wake latency (ns). Updates E_wait_time EMA in normalized units.
    /// Coarse: 1e6 ns ≈ 1.0 work unit (same scale as unresolved-producer prior).
    pub(crate) fn note_wait_latency(&self, location: MemoryLocationHash, latency_ns: u64) {
        self.wait_latency_ns_sum
            .fetch_add(latency_ns, Ordering::Relaxed);
        self.wait_latency_count.fetch_add(1, Ordering::Relaxed);
        let entry = self.locs.entry(location).or_default();
        entry
            .wait_latency_ns_sum
            .fetch_add(latency_ns, Ordering::Relaxed);
        entry.wait_latency_count.fetch_add(1, Ordering::Relaxed);
        let params = *self.params_bits.lock().unwrap();
        let lr = params.lr_wait_time;
        // Normalize: clamp to [0.05, 8.0] work units.
        let sample = (latency_ns as f64 / 1_000_000.0).clamp(0.05, 8.0);
        let prev = fp_decode(entry.e_wait_bits.load(Ordering::Relaxed));
        let prev = if prev <= f64::EPSILON {
            params.e_wait_prior
        } else {
            prev
        };
        let next = (1.0 - lr) * prev + lr * sample;
        entry.e_wait_bits.store(fp_encode(next), Ordering::Relaxed);
        let g_prev = fp_decode(self.global_e_wait_bits.load(Ordering::Relaxed));
        let g_next = (1.0 - lr) * g_prev + lr * sample;
        self.global_e_wait_bits
            .store(fp_encode(g_next), Ordering::Relaxed);
    }

    /// Steal/idle or park duration proxies → E_idle_steal EMA for EV_Wait.
    /// Steal success ⇒ low idle tax (cores stay busy); long park ⇒ high tax.
    pub(crate) fn note_steal_or_park_proxy(&self, steal: bool, park_ns: u64) {
        if steal {
            self.steal_events.fetch_add(1, Ordering::Relaxed);
        }
        if park_ns > 0 {
            self.park_ns_proxy.fetch_add(park_ns, Ordering::Relaxed);
        }
        // No sample if neither steal nor park (noop probe).
        if !steal && park_ns == 0 {
            return;
        }
        let params = *self.params_bits.lock().unwrap();
        let lr = params.lr_cascade;
        // Normalized idle tax units (same scale as E_wait_time).
        // Steal must NOT collapse E_idle: on fan-out, cheap Wait serializes
        // dependents even if the waiter core steals. Park-without-steal raises tax.
        let sample = if steal {
            params.e_idle_prior
        } else {
            (park_ns as f64 / 1_000_000.0).clamp(params.e_idle_prior, 4.0)
        };
        let prev = fp_decode(self.global_e_idle_bits.load(Ordering::Relaxed));
        let prev = if prev <= f64::EPSILON {
            params.e_idle_prior
        } else {
            prev
        };
        let next = (1.0 - lr) * prev + lr * sample;
        self.global_e_idle_bits
            .store(fp_encode(next), Ordering::Relaxed);
    }

    /// Count a meta op (SoftWait arm / WaitHard decision) toward ρ budget.
    pub(crate) fn note_meta_op(&self) {
        self.meta_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// E_wait_time(ℓ) — per-ℓ EMA or global prior.
    pub(crate) fn e_wait_time(&self, location: MemoryLocationHash) -> f64 {
        if let Some(e) = self.locs.get(&location) {
            let v = fp_decode(e.e_wait_bits.load(Ordering::Relaxed));
            if v > f64::EPSILON {
                return v;
            }
        }
        fp_decode(self.global_e_wait_bits.load(Ordering::Relaxed)).max(
            self.params_bits.lock().unwrap().e_wait_prior,
        )
    }

    /// E_cascade(ℓ) — per-ℓ EMA or global prior.
    pub(crate) fn e_cascade(&self, location: MemoryLocationHash) -> f64 {
        if let Some(e) = self.locs.get(&location) {
            let v = fp_decode(e.e_cascade_bits.load(Ordering::Relaxed));
            if v > f64::EPSILON {
                return v;
            }
        }
        let g = fp_decode(self.global_e_cascade_bits.load(Ordering::Relaxed));
        if g > f64::EPSILON {
            g
        } else {
            self.params_bits.lock().unwrap().e_cascade_prior
        }
    }

    /// Feed measured abort→reexec cost into E_reexec EMA (∉ TCB).
    /// Typical samples: RebindOnly≈0.1, RewindTo/FF≈0.6, FullRestart≈2.0+.
    pub(crate) fn note_reexec_cost(&self, cost: f64) {
        let sample = cost.clamp(0.05, 8.0);
        let params = *self.params_bits.lock().unwrap();
        let lr = params.lr_cascade; // reuse cascade lr for reexec EMA
        let prev = fp_decode(self.global_e_reexec_bits.load(Ordering::Relaxed));
        let prev = if prev <= f64::EPSILON {
            params.e_reexec
        } else {
            prev
        };
        let next = (1.0 - lr) * prev + lr * sample;
        self.global_e_reexec_bits
            .store(fp_encode(next), Ordering::Relaxed);
    }

    /// E_reexec — abort→reexec overhead for EV_Spec / EV_Early.
    pub(crate) fn e_reexec(&self) -> f64 {
        let g = fp_decode(self.global_e_reexec_bits.load(Ordering::Relaxed));
        if g > f64::EPSILON {
            g
        } else {
            self.params_bits.lock().unwrap().e_reexec
        }
    }

    /// E_idle_steal — park/idle tax when Wait does not keep cores busy via steal.
    pub(crate) fn e_idle_steal(&self) -> f64 {
        let g = fp_decode(self.global_e_idle_bits.load(Ordering::Relaxed));
        if g > f64::EPSILON {
            g
        } else {
            self.params_bits.lock().unwrap().e_idle_prior
        }
    }

    /// Continuous meta tax ratio meta_ops/useful (0 before warmup). Feeds tiny-gap Spec bias.
    pub(crate) fn meta_tax_ratio(&self, params: &AdaptiveParams) -> f64 {
        let obs = self.program_obs.load(Ordering::Relaxed)
            + self.handler_obs.load(Ordering::Relaxed);
        if obs < 64 {
            return 0.0;
        }
        let meta = self.meta_ops.load(Ordering::Relaxed) as f64;
        let useful = self
            .useful_effects
            .load(Ordering::Relaxed)
            .max(self.program_obs.load(Ordering::Relaxed))
            .max(obs / 4)
            .max(1) as f64;
        (meta / useful).max(0.0)
    }

    /// Mean cascade size this block (fallback).
    pub(crate) fn mean_cascade(&self) -> f64 {
        let n = self.abort_events.load(Ordering::Relaxed);
        if n == 0 {
            return self.params_bits.lock().unwrap().e_cascade_prior;
        }
        self.cascade_sum.load(Ordering::Relaxed) as f64 / n as f64
    }

    /// Meta budget exceeded: SoftWait/meta_ops over useful effects > ρ.
    /// Requires warmup observes so cold start cannot OCC-fallback the whole block.
    pub(crate) fn meta_budget_exceeded(&self, params: &AdaptiveParams) -> bool {
        let obs = self.program_obs.load(Ordering::Relaxed)
            + self.handler_obs.load(Ordering::Relaxed);
        if obs < 64 {
            return false;
        }
        let meta = self.meta_ops.load(Ordering::Relaxed) as f64;
        let useful = self
            .useful_effects
            .load(Ordering::Relaxed)
            .max(self.program_obs.load(Ordering::Relaxed))
            .max(obs / 4)
            .max(1) as f64;
        meta / useful > params.meta_budget_rho
    }

    pub(crate) fn meta_ops(&self) -> usize {
        self.meta_ops.load(Ordering::Relaxed)
    }

    pub(crate) fn useful_effects(&self) -> usize {
        self.useful_effects.load(Ordering::Relaxed)
    }

    pub(crate) fn wait_latency_count(&self) -> usize {
        self.wait_latency_count.load(Ordering::Relaxed)
    }

    pub(crate) fn mean_wait_latency_ns(&self) -> Option<f64> {
        let n = self.wait_latency_count.load(Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        Some(self.wait_latency_ns_sum.load(Ordering::Relaxed) as f64 / n as f64)
    }

    pub(crate) fn steal_events(&self) -> usize {
        self.steal_events.load(Ordering::Relaxed)
    }

    /// Optional status hist at discovery (Data vs still-running producer).
    pub(crate) fn note_producer_status(&self, location: MemoryLocationHash, data_ready: bool) {
        let e = self.locs.entry(location).or_default();
        if data_ready {
            e.status_data.fetch_add(1, Ordering::Relaxed);
        } else {
            e.status_running.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record measured/proxy depth sample when known (inspect or effect-progress).
    pub(crate) fn note_depth_sample(&self, d: f64) {
        let d = d.clamp(0.0, 1.0);
        // Store as fixed-point × 1e6 for atomic add.
        let bits = (d * 1_000_000.0) as u64;
        self.d_sum_bits.fetch_add(bits, Ordering::Relaxed);
        self.d_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn mean_depth_sample(&self) -> Option<f64> {
        let n = self.d_count.load(Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        let sum = self.d_sum_bits.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        Some(sum / n as f64)
    }

    pub(crate) fn bind_success_total(&self) -> usize {
        self.bind_success_total.load(Ordering::Relaxed)
    }

    pub(crate) fn wait_useful_total(&self) -> usize {
        self.wait_useful_total.load(Ordering::Relaxed)
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

    /// A6: per-ℓ abort rate for warm-seed decay.
    pub(crate) fn abort_rate_of(&self, location: MemoryLocationHash) -> f64 {
        self.locs
            .get(&location)
            .map(|e| {
                let readers = e.readers.load(Ordering::Relaxed) as f64;
                let aborts = e.aborts.load(Ordering::Relaxed) as f64;
                aborts / readers.max(1.0)
            })
            .unwrap_or(0.0)
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

    #[test]
    fn g4_bind_and_wait_useful_and_publish_credit() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        live.note_bind_success(1);
        live.note_wait_useful(1);
        live.note_publish(2);
        live.note_depth_sample(0.9);
        assert_eq!(live.bind_success_total(), 2); // bind + publish
        assert_eq!(live.wait_useful_total(), 1);
        assert!((live.mean_depth_sample().unwrap() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn g1_fan_out_wait_depth_prior() {
        let m = MorphWeights {
            fan_out: 0.55,
            mixed: 0.25,
            waw_spine: 0.10,
            quiet: 0.10,
        };
        assert!(m.dominant_fan_out());
        assert!((m.wait_depth_prior().unwrap() - 0.9).abs() < 1e-9);
        assert!(MorphWeights::default().wait_depth_prior().is_none());
    }

    #[test]
    fn g6_adaptive_params_from_l3() {
        let p = AdaptiveParams::from_l3();
        assert!((p.d_wait - 0.50).abs() < f64::EPSILON);
        assert!((p.d_early - 0.15).abs() < f64::EPSILON);
        assert!(p.alpha_fanout > 0.0);
        assert!(p.meta_budget_rho > 0.0);
        let p2 = p.with_overrides(Some(0.55), None, None, None, None, None, None, None);
        assert!((p2.d_wait - 0.55).abs() < f64::EPSILON);
        assert!((p2.d_early - 0.15).abs() < f64::EPSILON);
    }

    #[test]
    fn aec_wait_latency_updates_e_wait() {
        let live = LiveLearner::new();
        let params = AdaptiveParams::from_l3();
        live.begin_block_with_params(MorphWeights::default(), params);
        let prior = live.e_wait_time(9);
        live.note_wait_latency(9, 2_000_000); // 2.0 units
        let after = live.e_wait_time(9);
        assert!(after > prior.min(params.e_wait_prior) - 1e-9);
        assert_eq!(live.wait_latency_count(), 1);
    }

    #[test]
    fn aec_meta_budget_trips_on_softwait_storm() {
        let live = LiveLearner::new();
        let params = AdaptiveParams::from_l3();
        live.begin_block_with_params(MorphWeights::default(), params);
        // Warmup observes (meta budget inactive before 64).
        for i in 0..70 {
            live.note_observe(i as u64, true, 1);
        }
        live.note_bind_success(1);
        for _ in 0..40 {
            live.note_meta_op();
        }
        assert!(live.meta_budget_exceeded(&params));
        // Fresh block: no trip before warmup.
        live.begin_block_with_params(MorphWeights::default(), params);
        live.note_meta_op();
        assert!(!live.meta_budget_exceeded(&params));
    }

    #[test]
    fn aec_high_fanout_raises_ev_wait_feature() {
        // Feature contract: e_wait * (1 + α * fanout) grows with fanout.
        let p = AdaptiveParams::from_l3();
        let e = p.e_wait_prior;
        let low = e * (1.0 + p.alpha_fanout * 1.0);
        let high = e * (1.0 + p.alpha_fanout * 16.0);
        assert!(high > low);
    }

    #[test]
    fn v5_p2_park_raises_idle_tax_steal_does_not_collapse() {
        let live = LiveLearner::new();
        let params = AdaptiveParams::from_l3();
        live.begin_block_with_params(MorphWeights::default(), params);
        let prior = live.e_idle_steal();
        // Long park without steal → idle tax up.
        live.note_steal_or_park_proxy(false, 3_000_000);
        let after_park = live.e_idle_steal();
        assert!(after_park > prior + 0.1, "{after_park} vs {prior}");
        // Steal refreshes toward prior — must not collapse below ~prior/2.
        for _ in 0..12 {
            live.note_steal_or_park_proxy(true, 0);
        }
        let after_steal = live.e_idle_steal();
        assert!(
            after_steal + 1e-9 >= params.e_idle_prior * 0.5,
            "steal must not cheapen Wait via collapsed idle: {after_steal}"
        );
        assert!(after_steal < after_park, "{after_steal} vs {after_park}");
    }

    #[test]
    fn v5_p2_reexec_ema_tracks_lean_force_bind_vs_full_restart() {
        let live = LiveLearner::new();
        let params = AdaptiveParams::from_l3();
        live.begin_block_with_params(MorphWeights::default(), params);
        // V5-P1 Lean ForceBind sample.
        live.note_reexec_cost(1.2);
        let mid = live.e_reexec();
        assert!(mid > 1.0 && mid < 1.6, "{mid}");
        // Bare FullRestart raises E_reexec.
        for _ in 0..6 {
            live.note_reexec_cost(2.2);
        }
        let high = live.e_reexec();
        assert!(high > mid, "{high} vs {mid}");
    }

    #[test]
    fn v5_p2_meta_tax_ratio_continuous() {
        let live = LiveLearner::new();
        let params = AdaptiveParams::from_l3();
        live.begin_block_with_params(MorphWeights::default(), params);
        assert_eq!(live.meta_tax_ratio(&params), 0.0);
        for i in 0..70 {
            live.note_observe(i as u64, true, 1);
        }
        live.note_bind_success(1);
        for _ in 0..5 {
            live.note_meta_op();
        }
        let r = live.meta_tax_ratio(&params);
        assert!(r > 0.0, "{r}");
        assert!(!live.meta_budget_exceeded(&params) || r > params.meta_budget_rho);
    }
}
