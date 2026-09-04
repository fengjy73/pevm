//! Test-visible `SpecFence` counters. Updated atomically during a block.

use std::sync::atomic::{AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::BuildSuffixHasher;

/// Snapshot of `SpecFence` counters after a parallel block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpecFenceMetrics {
    /// Times a transaction was blocked because a Wait-mode region was not yet
    /// written by the prior consensus-order writer (proactive PCC admission).
    pub wait_admissions: usize,
    /// Successful executions that ran against Speculate (OCC) hinted accounts.
    pub speculate_executions: usize,
    /// Intra-block Speculate → Wait promotions (wave updates).
    pub region_promotions: usize,
    /// Validation aborts (OCC / SpecFence / PCC).
    pub occ_aborts: usize,
    /// Higher txs forced into the validation cascade by an abort rewind
    /// (`block_size - rewind_to` when a dependent reader exists).
    pub cascade_validations_scheduled: usize,
    /// Higher txs between `aborted_idx+1` and the first dependent reader that
    /// were **not** forced into the abort cascade (SpecFence fence).
    pub independent_txs_skipped_by_fence: usize,
    /// Bayesian decide() chose Wait for a location/account access.
    pub bayes_wait_decisions: usize,
    /// Bayesian decide() chose Speculate.
    pub bayes_speculate_decisions: usize,
    /// `observe_conflict` updates applied.
    pub bayes_conflict_updates: usize,
    /// `observe_speculate_ok` updates applied.
    pub bayes_success_updates: usize,
    /// Wave counter bumps when regions flip Speculate→Wait from bayes.
    pub wave_promotions: usize,
    /// Final wave id after the block.
    pub wave_id: usize,
    /// Mean conflict posterior among Wait decisions this block.
    pub mean_wait_posterior: f64,
    /// Accounts that triggered a Wait admission in this block.
    pub wait_addresses: Vec<Address>,
    /// `from`/`to` accounts of speculative executions in this block.
    pub speculate_addresses: Vec<Address>,
    /// Per-location validation failures.
    pub region_validate_fail: usize,
    /// FullRetry (whole-tx re-exec) counts.
    pub tx_full_retry: usize,
    /// Bind hits (read matched predicted writer).
    pub bind_hits: usize,
    /// WaitHard decisions / admissions at location grain.
    pub wait_hard_count: usize,
    /// SpecRead (OrderedDirtyRead) path counts.
    pub spec_read_count: usize,
    /// Selective invalidate applications.
    pub selective_invalidate_count: usize,
    /// Cascade revalidations scheduled (alias tracking for Spec v1 metrics).
    pub cascade_revalidate_count: usize,
    /// Soft Wait flags cleared because posterior < τ_revoke.
    pub soft_edge_revokes: usize,
    /// Times selective invalidate fell back to full write-set ESTIMATE.
    pub selective_fallback_full: usize,
    /// Checkpoint opportunities recorded (Phase-2 prep).
    pub checkpoint_opportunities: usize,
    /// Semantic PartialRetry applications (certified-prefix Bind on reexec).
    pub partial_retry_count: usize,
    /// PartialRetry attempted but fell back to FullRetry (unsafe split).
    pub partial_retry_fallback_full: usize,
    /// Cost-aware π chose WaitHard.
    pub cost_chose_wait: usize,
    /// Cost-aware π chose SpecRead.
    pub cost_chose_spec: usize,
    /// Cost-aware π chose Bind.
    pub cost_chose_bind: usize,
    /// Mean P_conflict among cost-aware WaitHard decisions.
    pub mean_p_at_wait: f64,
    /// Mean P_conflict among cost-aware SpecRead decisions.
    pub mean_p_at_spec: f64,
    /// Plant v2 M0: fresh EVM/transact/interpreter starts (new incarnation from tx head).
    /// Incremented at `Vm::execute` immediately before the handler `run` (OCC + SpecFence).
    /// Baseline today: ≈ n_tx + head-reexecs (PartialRetry and FullRetry both restart from head).
    pub evm_entries: usize,
    /// M1+: resume from checkpoint without fresh interpreter start (stays 0 until RewindTo).
    pub resume_count: usize,
    /// M1+: rebind-only repair without rewind/restart (stays 0 until Rebind).
    pub rebind_only: usize,
    /// M1+: rewind journal/PC to checkpoint then resume (stays 0 until RewindTo).
    pub rewind_to_cp: usize,
    /// FullRestart decisions: OCC abort reexec, or SpecFence FullRetry (no certified prefix).
    /// Each corresponding reexec also increments `evm_entries` at the next `Vm::execute`.
    pub full_restart: usize,
    /// Semantic PartialRetry (and EarlyVal force-bind) that still restarts the interpreter
    /// from tx head — not an L1 resume. Documented alias for "head reexec under PartialRetry".
    /// M1 RewindTo must NOT increment this; use `resume_count` / `rewind_to_cp` instead.
    pub tx_head_reexec: usize,
}

/// Shared counters written by worker threads.
#[derive(Debug, Default)]
pub(crate) struct MetricsInner {
    wait_admissions: AtomicUsize,
    speculate_executions: AtomicUsize,
    region_promotions: AtomicUsize,
    occ_aborts: AtomicUsize,
    cascade_validations_scheduled: AtomicUsize,
    independent_txs_skipped_by_fence: AtomicUsize,
    bayes_wait_decisions: AtomicUsize,
    bayes_speculate_decisions: AtomicUsize,
    bayes_conflict_updates: AtomicUsize,
    bayes_success_updates: AtomicUsize,
    wave_promotions: AtomicUsize,
    region_validate_fail: AtomicUsize,
    tx_full_retry: AtomicUsize,
    bind_hits: AtomicUsize,
    wait_hard_count: AtomicUsize,
    spec_read_count: AtomicUsize,
    selective_invalidate_count: AtomicUsize,
    cascade_revalidate_count: AtomicUsize,
    soft_edge_revokes: AtomicUsize,
    selective_fallback_full: AtomicUsize,
    checkpoint_opportunities: AtomicUsize,
    partial_retry_count: AtomicUsize,
    partial_retry_fallback_full: AtomicUsize,
    cost_chose_wait: AtomicUsize,
    cost_chose_spec: AtomicUsize,
    cost_chose_bind: AtomicUsize,
    evm_entries: AtomicUsize,
    resume_count: AtomicUsize,
    rebind_only: AtomicUsize,
    rewind_to_cp: AtomicUsize,
    full_restart: AtomicUsize,
    tx_head_reexec: AtomicUsize,
    wait_addresses: DashMap<Address, (), BuildSuffixHasher>,
    speculate_addresses: DashMap<Address, (), BuildSuffixHasher>,
    hot_accounts: DashMap<Address, (), BuildSuffixHasher>,
}

impl MetricsInner {
    pub(crate) fn record_wait(&self, address: Address) {
        self.wait_admissions.fetch_add(1, Ordering::Relaxed);
        self.wait_addresses.insert(address, ());
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn record_speculate(&self, from: Address, to: Option<Address>) {
        self.speculate_executions.fetch_add(1, Ordering::Relaxed);
        self.speculate_addresses.insert(from, ());
        if let Some(to) = to {
            self.speculate_addresses.insert(to, ());
        }
    }

    pub(crate) fn record_promotion(&self, address: Option<Address>) {
        self.region_promotions.fetch_add(1, Ordering::Relaxed);
        if let Some(address) = address {
            self.hot_accounts.insert(address, ());
        }
    }

    pub(crate) fn record_occ_abort(&self) {
        self.occ_aborts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fence_cascade(
        &self,
        cascade_scheduled: usize,
        independent_skipped: usize,
    ) {
        if cascade_scheduled > 0 {
            self.cascade_validations_scheduled
                .fetch_add(cascade_scheduled, Ordering::Relaxed);
            self.cascade_revalidate_count
                .fetch_add(cascade_scheduled, Ordering::Relaxed);
        }
        if independent_skipped > 0 {
            self.independent_txs_skipped_by_fence
                .fetch_add(independent_skipped, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_bayes_wait(&self) {
        self.bayes_wait_decisions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_speculate(&self) {
        self.bayes_speculate_decisions
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_conflict(&self) {
        self.bayes_conflict_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_success(&self) {
        self.bayes_success_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wave_promotion(&self) {
        self.wave_promotions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_region_validate_fail(&self, n: usize) {
        if n > 0 {
            self.region_validate_fail.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_tx_full_retry(&self) {
        self.tx_full_retry.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bind_hit(&self) {
        self.bind_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wait_hard(&self) {
        self.wait_hard_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_spec_read(&self) {
        self.spec_read_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_selective_invalidate(&self, n: usize) {
        if n > 0 {
            self.selective_invalidate_count
                .fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_soft_edge_revoke(&self) {
        self.soft_edge_revokes.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_selective_fallback_full(&self) {
        self.selective_fallback_full.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_checkpoint_opportunity(&self) {
        self.checkpoint_opportunities
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_checkpoint_opportunities(&self, n: usize) {
        self.checkpoint_opportunities.store(n, Ordering::Relaxed);
    }

    pub(crate) fn record_partial_retry(&self) {
        self.partial_retry_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_partial_retry_fallback_full(&self) {
        self.partial_retry_fallback_full
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_wait(&self) {
        self.cost_chose_wait.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_spec(&self) {
        self.cost_chose_spec.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_bind(&self) {
        self.cost_chose_bind.fetch_add(1, Ordering::Relaxed);
    }

    /// Fresh EVM session / interpreter start from tx head (plant v2 L1 denominator).
    pub(crate) fn record_evm_entry(&self) {
        self.evm_entries.fetch_add(1, Ordering::Relaxed);
    }

    /// M1: resume from checkpoint without counting as fresh tx-head entry.
    pub(crate) fn record_resume(&self) {
        self.resume_count.fetch_add(1, Ordering::Relaxed);
    }

    /// M1: rebind-only repair without rewind/restart.
    pub(crate) fn record_rebind_only(&self) {
        self.rebind_only.fetch_add(1, Ordering::Relaxed);
    }

    /// M1: rewind journal/PC to checkpoint then resume.
    pub(crate) fn record_rewind_to_cp(&self) {
        self.rewind_to_cp.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_full_restart(&self) {
        self.full_restart.fetch_add(1, Ordering::Relaxed);
    }

    /// Today's PartialRetry / EarlyVal still restarts interpreter from tx head.
    pub(crate) fn record_tx_head_reexec(&self) {
        self.tx_head_reexec.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn mark_hot(&self, address: Address) {
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn hot_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.hot_accounts.iter().map(|entry| *entry.key())
    }

    pub(crate) fn snapshot(
        &self,
        wave_id: usize,
        mean_wait_posterior: f64,
        mean_p_at_wait: f64,
        mean_p_at_spec: f64,
    ) -> SpecFenceMetrics {
        let mut wait_addresses: Vec<Address> =
            self.wait_addresses.iter().map(|e| *e.key()).collect();
        wait_addresses.sort_unstable();
        let mut speculate_addresses: Vec<Address> =
            self.speculate_addresses.iter().map(|e| *e.key()).collect();
        speculate_addresses.sort_unstable();
        SpecFenceMetrics {
            wait_admissions: self.wait_admissions.load(Ordering::Relaxed),
            speculate_executions: self.speculate_executions.load(Ordering::Relaxed),
            region_promotions: self.region_promotions.load(Ordering::Relaxed),
            occ_aborts: self.occ_aborts.load(Ordering::Relaxed),
            cascade_validations_scheduled: self
                .cascade_validations_scheduled
                .load(Ordering::Relaxed),
            independent_txs_skipped_by_fence: self
                .independent_txs_skipped_by_fence
                .load(Ordering::Relaxed),
            bayes_wait_decisions: self.bayes_wait_decisions.load(Ordering::Relaxed),
            bayes_speculate_decisions: self.bayes_speculate_decisions.load(Ordering::Relaxed),
            bayes_conflict_updates: self.bayes_conflict_updates.load(Ordering::Relaxed),
            bayes_success_updates: self.bayes_success_updates.load(Ordering::Relaxed),
            wave_promotions: self.wave_promotions.load(Ordering::Relaxed),
            wave_id,
            mean_wait_posterior,
            wait_addresses,
            speculate_addresses,
            region_validate_fail: self.region_validate_fail.load(Ordering::Relaxed),
            tx_full_retry: self.tx_full_retry.load(Ordering::Relaxed),
            bind_hits: self.bind_hits.load(Ordering::Relaxed),
            wait_hard_count: self.wait_hard_count.load(Ordering::Relaxed),
            spec_read_count: self.spec_read_count.load(Ordering::Relaxed),
            selective_invalidate_count: self.selective_invalidate_count.load(Ordering::Relaxed),
            cascade_revalidate_count: self.cascade_revalidate_count.load(Ordering::Relaxed),
            soft_edge_revokes: self.soft_edge_revokes.load(Ordering::Relaxed),
            selective_fallback_full: self.selective_fallback_full.load(Ordering::Relaxed),
            checkpoint_opportunities: self.checkpoint_opportunities.load(Ordering::Relaxed),
            partial_retry_count: self.partial_retry_count.load(Ordering::Relaxed),
            partial_retry_fallback_full: self
                .partial_retry_fallback_full
                .load(Ordering::Relaxed),
            cost_chose_wait: self.cost_chose_wait.load(Ordering::Relaxed),
            cost_chose_spec: self.cost_chose_spec.load(Ordering::Relaxed),
            cost_chose_bind: self.cost_chose_bind.load(Ordering::Relaxed),
            mean_p_at_wait,
            mean_p_at_spec,
            evm_entries: self.evm_entries.load(Ordering::Relaxed),
            resume_count: self.resume_count.load(Ordering::Relaxed),
            rebind_only: self.rebind_only.load(Ordering::Relaxed),
            rewind_to_cp: self.rewind_to_cp.load(Ordering::Relaxed),
            full_restart: self.full_restart.load(Ordering::Relaxed),
            tx_head_reexec: self.tx_head_reexec.load(Ordering::Relaxed),
        }
    }
}
