use alloy_primitives::{Address, B256, TxKind, U256};
use alloy_rpc_types_eth::Receipt;
use hashbrown::HashMap;
use revm::{
    Database,
    context::{
        BlockEnv, ContextSetters, ContextTr, DBErrorMarker, JournalTr, TxEnv,
        result::{EVMError, ExecutionResult, InvalidTransaction},
    },
    handler::EvmTr,
    primitives::KECCAK_EMPTY,
    state::{AccountInfo, Bytecode, EvmState},
};
use smallvec::SmallVec;
use std::cell::Cell;
use std::time::Instant;

use crate::{
    AccountBasic, BuildIdentityHasher, BuildSuffixHasher, EvmAccount, FinishExecFlags, MemoryEntry,
    MemoryLocation, MemoryLocationHash, MemoryValue, ReadOrigin, ReadOrigins, ReadSet, Storage,
    TxIdx, TxVersion, WriteSet,
    chain::PevmChain,
    hash_deterministic,
    mv_memory::MvMemory,
    specfence::{
        AccessDecision, AccessMode, AccessVis, CheckpointKind, DecisionFeat, DecisionVerb, EdgeKey,
        EdgeKind, EdgeState, FfValue, OrderedAdmitSnapMode, ProcessReason, SpecFenceCtx,
        StorageWriteReplay, VisibilityPolicy, absolute_jump_eligible, arm_call_outcome_cache,
        arm_ff_origin_seeds, attach_current_live_snap, early_val_probability, jump_is_safe,
        jump_refuse_reason, note_pending_effect_boundary, note_pending_ordered_admit_snap,
        ordered_admit_snap_jump_enabled, ordered_admit_snap_mode, resume_was_applied,
        steps_this_run, suffix_repair_jump_env_ok, take_ff_origin_seeds,
        try_arm_safe_absolute_jump, try_arm_safe_absolute_jump_gated, with_ordered_admit_snap_tls,
        with_protocol_tls_journal,
    },
};

/// The execution error from the underlying EVM executor.
// Will there be DB errors outside of read?
pub type ExecutionError = EVMError<ReadError>;

/// Represents the state transitions of the EVM accounts after execution.
/// If the value is [None], it indicates that the account is marked for removal.
/// If the value is [`Some(new_state)`], it indicates that the account has become [`new_state`].
type EvmStateTransitions = HashMap<Address, Option<EvmAccount>, BuildSuffixHasher>;

/// Execution result of a transaction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PevmTxExecutionResult {
    /// Receipt of execution
    // TODO: Consider promoting to [ReceiptEnvelope] if there is high demand
    pub receipt: Receipt,
    /// State that got updated
    pub state: EvmStateTransitions,
}

/// Convert Revm's execution result into a standard receipt.
/// Note that the cumulative gas used in the receipt is preset to the gas used in this transaction.
/// It should be post-processed with the remaining transactions in the block.
pub(crate) fn receipt_from_revm<H>(result: ExecutionResult<H>) -> Receipt {
    Receipt {
        status: result.is_success().into(),
        cumulative_gas_used: result.tx_gas_used(),
        logs: result.into_logs(),
    }
}

/// Convert Revm's state transitions into PEVM's state transitions.
pub(crate) fn state_transitions_from_revm(
    is_eip_161_enabled: bool,
    state: EvmState,
) -> impl Iterator<Item = (Address, Option<EvmAccount>)> {
    state
        .into_iter()
        .filter(|(_, account)| account.is_touched())
        .map(move |(address, account)| {
            if account.is_selfdestructed() || account.is_empty() && is_eip_161_enabled {
                (address, None)
            } else {
                (address, Some(EvmAccount::from(account)))
            }
        })
}

pub(crate) enum VmExecutionError {
    Retry,
    FallbackToSequential,
    Blocking(TxIdx),
    ExecutionError(ExecutionError),
}

/// Errors when reading a memory location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// Cannot read memory location from storage.
    // TODO: More concrete type
    #[error("Failed reading memory from storage: {0}")]
    StorageError(String),
    /// This memory location has been written by a lower transaction.
    #[error("Read of memory location is blocked by tx #{0}")]
    Blocking(TxIdx),
    /// There has been an inconsistent read like reading the same
    /// location from storage in the first call but from [`VmMemory`] in
    /// the next.
    #[error("Inconsistent read")]
    InconsistentRead,
    /// Found an invalid nonce, like the first transaction of a sender
    /// not having a (+1) nonce from storage.
    #[error("Tx #{0} has invalid nonce")]
    InvalidNonce(TxIdx),
    /// Read a self-destructed account that is very hard to handle, as
    /// there is no performant way to mark all storage slots as cleared.
    #[error("Tried to read self-destructed account")]
    SelfDestructedAccount,
    /// The stored memory value type doesn't match its location type.
    // TODO: Handle this at the type level?
    #[error("Invalid type of stored memory value")]
    InvalidMemoryValueType,
}

impl DBErrorMarker for ReadError {}

impl From<ReadError> for VmExecutionError {
    fn from(err: ReadError) -> Self {
        match err {
            ReadError::InconsistentRead => Self::Retry,
            ReadError::SelfDestructedAccount => Self::FallbackToSequential,
            ReadError::Blocking(tx_idx) => Self::Blocking(tx_idx),
            _ => Self::ExecutionError(EVMError::Database(err)),
        }
    }
}

// A database interface that intercepts reads while executing a specific
// transaction with Revm. It provides values from the multi-version data
// structure & storage, and tracks the read set of the current execution.
pub(crate) struct VmDb<'a, S: Storage> {
    storage: &'a S,
    mv_memory: &'a MvMemory,
    specfence: SpecFenceCtx<'a>,
    tx_idx: TxIdx,
    tx_incarnation: crate::TxIncarnation,
    tx: &'a TxEnv,
    from_hash: MemoryLocationHash,
    to_hash: Option<MemoryLocationHash>,
    to_code_hash: Option<B256>,
    // Indicates if we lazy update this transaction.
    // Only applied to raw transfers' senders & recipients at the moment.
    is_lazy: bool,
    /// Thin-shell short same-from/to force-lazy (not the generic first-touch lazy).
    optimistic_majority_lazy: bool,
    /// A1=0 ungated: skip access-gate / rem / ReadyEdge (P3 ≡ OCC).
    optimistic_skip_gate: bool,
    /// Thin Soft=0: no WaitOnce peer before this tx → OCC-shaped execute
    /// (skip consult body / engagement / museum; keep access_log ordinals).
    sf_occ_shaped: bool,
    /// SpecFence visibility for this incarnation (Opt / WaitReleased / OrderedTip).
    vis: VisibilityPolicy,
    /// PCC overlay armed for the current access (PE ∩ ROI). OptimisticRead = OCC read.
    pcc_armed: Cell<bool>,
    /// OptimisticRead accesses this incarnation (end-tx process flush; no DashMap).
    optimistic_read_this_tx: Cell<u32>,
    pcc_this_tx: Cell<u32>,
    // Whether to enforce the sender-nonce ordering check for this transaction.
    // False for transaction types with no nonce (e.g. OP deposits).
    has_nonce: bool,
    read_set: ReadSet,
    // TODO: Clearer type for [AccountBasic] plus code hash
    read_accounts: HashMap<MemoryLocationHash, (AccountBasic, Option<B256>), BuildIdentityHasher>,
}

impl<'a, S: Storage> VmDb<'a, S> {
    // Reset per-transaction fields for allocation reuse.
    // Must be called before each transaction execution.
    fn set_tx(
        &mut self,
        tx_idx: TxIdx,
        tx: &'a TxEnv,
        from_hash: MemoryLocationHash,
        to_hash: Option<MemoryLocationHash>,
        has_nonce: bool,
        incarnation: crate::TxIncarnation,
    ) -> Result<(), ReadError> {
        self.tx_idx = tx_idx;
        self.tx_incarnation = incarnation;
        self.tx = tx;
        self.from_hash = from_hash;
        self.to_hash = to_hash;
        self.to_code_hash = None;
        self.flush_access_census();
        self.is_lazy = false;
        self.optimistic_majority_lazy = false;
        self.optimistic_skip_gate = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && self
                .specfence
                .policy
                .is_some_and(|p| p.skip_ungated_tx_path_tax())
            && !self.specfence.ready_edges.is_gated(tx_idx);
        // Thin Soft=0 OCC-shaped: no WaitOnce peer before this tx → skip consult
        // body / engagement / museum. Keep access_log.note (fail_k). Tip install
        // still runs if this tx is a WaitOnce/crit producer.
        self.sf_occ_shaped = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && self.specfence.scheduler.block_size() <= crate::specfence::THIN_SHELL_N
            && !self
                .specfence
                .access_arms
                .has_wait_once_peer_before(tx_idx);
        self.vis = if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            // leftover_min → WaitReleased (for_ready). Worker vis was
            // discarded here and leftover_min stayed Opt (19807137 ghost).
            VisibilityPolicy::for_ready(self.specfence.ready_edges, tx_idx)
        } else {
            VisibilityPolicy::Opt
        };
        self.has_nonce = has_nonce;
        self.read_set.clear();
        self.read_accounts.clear();
        self.pcc_armed.set(false);
        if let Some(fg) = self.specfence.finegrain {
            // OCC-shaped: no finegrain museum (no rem / consult grain).
            if !self.sf_occ_shaped {
                fg.deep_begin_consumer(tx_idx, incarnation);
            }
        }
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            // Every incarnation, including the thin ungated shell. Otherwise
            // fail_k is unknown and prefix keep cannot see this read.
            self.specfence.access_log.begin_incarnation(tx_idx);
        }
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence && !self.optimistic_skip_gate {
            // repair_armed covers_all only with real FF values. WaitForDependency ResumeAtK
            // with empty FF must not pretend sibling optimistic_read is certified (Iter26).
            let repair_armed = (self.specfence.partial_retry.is_rewind_resume(tx_idx)
                || self.specfence.partial_retry.has_ff_head(tx_idx))
                && self.specfence.partial_retry.has_ff_resume_values(tx_idx);
            self.specfence
                .certificates
                .begin_execute(tx_idx, repair_armed, incarnation);
            self.specfence
                .partial_retry
                .reset_incarnation(tx_idx, incarnation);
            // FF replay is a prefix certificate — Spec-only incarnations never arm rem.
            if self.specfence.certificates.repair_armed(tx_idx) {
                let n = self.specfence.partial_retry.replay_ff_if_armed(tx_idx);
                if n > 0 {
                    self.specfence.metrics.record_journal_ff_entries(n);
                }
            }
        } else if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            // Ungated shell still drops the previous incarnation's snaps.
            // ff_head (prefix keep) is not in that cell.
            // OCC-shaped: no value_snap / rem grain this tx — skip reset.
            if !self.sf_occ_shaped {
                self.specfence
                    .partial_retry
                    .reset_incarnation(tx_idx, incarnation);
            }
        }
        if let TxKind::Call(to) = tx.kind {
            self.to_code_hash = self.get_code_hash(to)?;

            // We only lazy update raw transfers that already have the sender
            // or recipient in [MvMemory] since sequentially evaluating memory
            // locations with only one entry is much costlier than fully
            // evaluating it concurrently.
            // TODO: Only lazy update in block syncing mode, not for block
            // building.
            let eoa = self.to_code_hash.is_none();
            let already = eoa
                && (self.mv_memory.data.contains_key(&from_hash)
                    || self.mv_memory.data.contains_key(&to_hash.unwrap()));
            // A0-majority: also lazy-accumulate empty-input EOA that share
            // from/to with another tx (2-tx same-from 21k). Avoids first-touch
            // Basic WAW without a ReadyEdge on the whole spine.
            let optimistic_majority_lazy = eoa
                && self.specfence.mode == crate::ConcurrencyMode::SpecFence
                && crate::specfence::optimistic_majority_hinted_lazy(
                    self.specfence.hints,
                    tx.caller,
                    Some(to),
                    tx.data.is_empty(),
                    true,
                    self.specfence
                        .policy
                        .is_some_and(|p| p.is_optimistic_majority_block()),
                    self.specfence.ready_edges.was_queued(tx_idx),
                );
            self.optimistic_majority_lazy = optimistic_majority_lazy;
            // Detect same-to fan-in (≥2) is lazy even when the majority-block
            // flag is still cold. Otherwise a higher-idx first-touch Basic
            // resets the lazy evaluation chain (iter11 24×hot).
            let to_fanin = eoa
                && self.specfence.mode == crate::ConcurrencyMode::SpecFence
                && self.specfence.hints.to_txs(&to).len() >= 2;
            self.is_lazy = already || optimistic_majority_lazy || to_fanin;
            if optimistic_majority_lazy && self.specfence.hints.prev(&tx.caller, tx_idx).is_some() {
                self.specfence.metrics.record_commute_skip();
                if let Some(p) = self.specfence.policy {
                    p.note_commute_skip();
                    p.ignore_conflict(Some(from_hash));
                }
            }
        }
        Ok(())
    }

    /// M1b: try serving a certified-prefix read from the FF value cache.
    /// Returns Some when origin is unchanged — caller skips MV lazy walk.

    /// Record rem value_snap for value-stable RebindOnly at validate (and journal FF).
    /// Thin OCC-shaped (no WaitOnce peer): rem unused — thin skips Rewind and
    /// shaped txs do not arm prefix keep; snap inserts are pure shell tax.
    #[inline]
    fn maybe_note_value(&self, location_hash: MemoryLocationHash, value: FfValue) {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence || self.sf_occ_shaped {
            return;
        }
        self.specfence
            .partial_retry
            .note_value(self.tx_idx, location_hash, value);
    }

    fn try_ff_storage(&self, location_hash: MemoryLocationHash) -> Option<(U256, ReadOrigin)> {
        // Iter5: RewindTo resume OR escalate head-FF retain.
        if !self.specfence.partial_retry.is_rewind_resume(self.tx_idx)
            && !self.specfence.partial_retry.has_ff_head(self.tx_idx)
        {
            return None;
        }
        let FfValue::Storage { value, origin, .. } = self
            .specfence
            .partial_retry
            .ff_value(self.tx_idx, location_hash)?
        else {
            return None;
        };
        let current = self
            .mv_memory
            .last_data_before(location_hash, self.tx_idx)
            .map(|(tx_idx, tx_incarnation)| (tx_idx, tx_incarnation));
        if current == origin {
            let read_origin = match origin {
                Some((tx_idx, tx_incarnation)) => ReadOrigin::MvMemory(TxVersion {
                    tx_idx,
                    tx_incarnation,
                }),
                None => ReadOrigin::Storage,
            };
            // Iter26: FF tip≡FF by construction — arm OrderedAdmit-snap. Attach deferred
            // to TLS exit (one bsnap/resume). Jump keeps Validated-prefix spin.
            note_pending_ordered_admit_snap();
            return Some((value, read_origin));
        }
        // Iter13: Validated-gated value-stable FF (origin bump, same U256).
        // Iter10 bare value-stable FF falsified (livelock / N10 wall↑) — only
        // after writer Validated. Rebind read origin to current Validated tip.
        // FF-path Validated yield falsified (13c no wall win). SoftWait Soft=0.
        let (w_idx, w_inc) = current?;
        if !self.specfence.scheduler.is_validated(w_idx) {
            return None;
        }
        let _nest = crate::mv_memory::DataNest::enter("try_ff_storage");
        let written = self.mv_memory.data.get(&location_hash)?;
        let MemoryEntry::Data(inc, MemoryValue::Storage(cur_v)) = written.get(&w_idx)? else {
            return None;
        };
        if *inc != w_inc || *cur_v != value {
            return None;
        }
        self.specfence.metrics.record_value_stable_ff_hit();
        // Iter26: value-stable Validated FF — same tip≡FF arm (same Validated window).
        note_pending_ordered_admit_snap();
        Some((
            value,
            ReadOrigin::MvMemory(TxVersion {
                tx_idx: w_idx,
                tx_incarnation: w_inc,
            }),
        ))
    }

    fn try_ff_basic(
        &self,
        location_hash: MemoryLocationHash,
    ) -> Option<(AccountBasic, Option<B256>, ReadOrigin)> {
        if !self.specfence.partial_retry.is_rewind_resume(self.tx_idx)
            && !self.specfence.partial_retry.has_ff_head(self.tx_idx)
        {
            return None;
        }
        let FfValue::Basic {
            basic,
            code_hash,
            origin,
            ..
        } = self
            .specfence
            .partial_retry
            .ff_value(self.tx_idx, location_hash)?
        else {
            return None;
        };
        // Only single-origin basics are cached; require matching top writer
        // OR Iter13 Validated-gated value-stable (balance+nonce; code via hash).
        // Iter10 bare Basic+Storage livelocked — Validated gate only.
        let current = self
            .mv_memory
            .last_data_before(location_hash, self.tx_idx)
            .map(|(tx_idx, tx_incarnation)| (tx_idx, tx_incarnation));
        if current == origin {
            let read_origin = match origin {
                Some((tx_idx, tx_incarnation)) => ReadOrigin::MvMemory(TxVersion {
                    tx_idx,
                    tx_incarnation,
                }),
                None => ReadOrigin::Storage,
            };
            return Some((basic, code_hash, read_origin));
        }
        let (w_idx, w_inc) = current?;
        if !self.specfence.scheduler.is_validated(w_idx) {
            return None;
        }
        let _nest = crate::mv_memory::DataNest::enter("try_ff_basic");
        let written = self.mv_memory.data.get(&location_hash)?;
        let MemoryEntry::Data(inc, MemoryValue::Basic(cur_b)) = written.get(&w_idx)? else {
            return None;
        };
        if *inc != w_inc {
            return None;
        }
        if cur_b.balance != basic.balance || cur_b.nonce != basic.nonce {
            return None;
        }
        self.specfence.metrics.record_value_stable_ff_hit();
        Some((
            basic,
            code_hash,
            ReadOrigin::MvMemory(TxVersion {
                tx_idx: w_idx,
                tx_incarnation: w_inc,
            }),
        ))
    }

    fn hash_basic(&self, address: &Address) -> MemoryLocationHash {
        if address == &self.tx.caller {
            return self.from_hash;
        }
        if let TxKind::Call(to) = &self.tx.kind
            && to == address
        {
            return self.to_hash.unwrap();
        }
        hash_deterministic(MemoryLocation::Basic(*address))
    }

    fn promote_on_conflict(&self, address: Address, location: MemoryLocationHash) {
        if !self.specfence.mode.uses_regions() || address == self.specfence.beneficiary {
            return;
        }
        // Intra-block Wait; at most one Bayes conflict obs per location per block
        // (validation aborts still call observe_conflict_location_always).
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            if self.specfence.bayes.observe_conflict_location(location) {
                self.specfence.metrics.record_bayes_conflict();
                if address != self.specfence.beneficiary {
                    self.specfence.bayes.observe_conflict_account_n(address, 3);
                }
            }
            self.specfence
                .promote_from_bayes(&self.mv_memory.regions, location, Some(address));
            return;
        }
        if self.mv_memory.regions.promote_location(location) {
            self.specfence.metrics.record_promotion(Some(address));
        }
        // G5: account promote is PCC/legacy only (SpecFence returned above).
        self.mv_memory.regions.promote_account(address);
        self.specfence.metrics.mark_hot(address);
    }

    /// End-tx OptimisticRead census (no per-SLOAD process DashMap).
    fn flush_access_census(&self) {
        let n = self.optimistic_read_this_tx.get();
        if n > 0
            && self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && !self.optimistic_skip_gate
        {
            self.specfence
                .process
                .note_optimistic_read_occ(self.tx_idx, n);
        }
        self.optimistic_read_this_tx.set(0);
        self.pcc_this_tx.set(0);
        self.pcc_armed.set(false);
    }

    /// Learned WaitOnce on the ungated Opt path. Consume via SfMvMemory
    /// read-after-true-publish (version tip / Data) — never Estimate Block.
    /// Large: park a live / SF-tipped unfinished pred without mark_gated.
    /// Thin: micro-spin while Executing; exact-waiter defer only on SF tip.
    fn consult_ungated_wait_once(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        access_k: u32,
    ) -> Result<(), ReadError> {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence {
            return Ok(());
        }
        if self.is_lazy || address == self.specfence.beneficiary {
            return Ok(());
        }
        // Thin Soft=0 OCC-shaped: no WaitOnce peer — zero consult tax (ordinals
        // already noted by basic/storage). A mid-block protected ℓ is the
        // exception: that access waits for the true tip instead of Opt.
        if self.sf_occ_shaped
            && !(self.specfence.access_arms.protect_live()
                && self.specfence.access_arms.is_protected(location_hash))
        {
            return Ok(());
        }
        let protected = self.specfence.access_arms.is_protected(location_hash);
        // Before the Opt read. Large is not OCC-shaped, so this cannot sit
        // behind the thin shaped check.
        if protected {
            self.specfence.access_arms.note_protect_before_opt();
        }
        // Soft=0 Opt: no WaitOnce and no crit → zero consult tax.
        if !self.specfence.access_arms.any_wait_once()
            && self.specfence.access_arms.crit_loc_hash() == u64::MAX
        {
            return Ok(());
        }
        let wait = self.specfence.access_arms.is_wait_once(location_hash);
        let crit = self.specfence.access_arms.crit_loc_hash() == location_hash;
        if !wait && !crit {
            return Ok(());
        }
        let thin = self.specfence.scheduler.block_size() <= crate::specfence::THIN_SHELL_N;
        // Checkpoint before this read so fail_k can RewindTo (large blocks).
        // Thin shell skips RewindTo — do not pay rem tax for unused cps.
        if !thin && access_k > 1 {
            let _ = self.specfence.partial_retry.push_checkpoint_at_k(
                self.tx_idx,
                (access_k - 1) as usize,
                crate::specfence::CheckpointKind::EffectBoundary,
            );
        }
        if !wait {
            return Ok(());
        }
        let Some(pred) = self
            .specfence
            .access_arms
            .wait_once_pred(self.tx_idx, location_hash)
            .or_else(|| {
                self.specfence
                    .access_arms
                    .crit_pred(self.tx_idx, location_hash)
            })
            .or_else(|| {
                // Thin Soft=0: Learn peer + crit are enough — skip MvMemory /
                // writers_of alloc ladder (scaffolding tax when Avoid already works).
                // A protected ℓ has peer 0; the ordered writer is the true tip.
                if thin && !protected {
                    return None;
                }
                self.mv_memory
                    .last_writer_before(location_hash, self.tx_idx)
            })
            .or_else(|| {
                self.specfence
                    .sf_tips
                    .live_writer(location_hash)
                    .filter(|&w| w < self.tx_idx)
            })
            .or_else(|| {
                if thin && !protected {
                    return None;
                }
                self.specfence
                    .ready_edges
                    .writers_of(location_hash)
                    .into_iter()
                    .rev()
                    .find(|&w| w < self.tx_idx)
            })
        else {
            return Ok(());
        };
        if pred >= self.tx_idx {
            return Ok(());
        }
        // Four-class: Chain (sticky≥32) | WAW (early-k) | RAW (else WaitOnce).
        let class = crate::specfence::classify_wait_conflict(
            self.specfence.access_arms.is_crit_loc(location_hash),
            self.specfence.access_arms.crit_chain_len(),
            self.specfence.access_arms.wait_once_k(location_hash),
        );
        // (a) Detect before read — concurrent capability, not a later stage.
        self.specfence.sf_tips.record_detect_before();
        let finished =
            self.specfence.scheduler.is_done(pred) || self.specfence.scheduler.is_validated(pred);
        let sf = crate::specfence::SfMvMemory::new(self.mv_memory, self.specfence.sf_tips);
        if finished || sf.true_publish_ready(location_hash, pred) {
            // (b) Avoid at read via true publish / done.
            self.specfence.sf_tips.record_avoid_publish();
            self.specfence.sf_tips.record_class_avoid(class);
            return Ok(());
        }
        let executing = self.specfence.scheduler.is_executing(pred);
        let has_sf_tip = self
            .specfence
            .sf_tips
            .has_version_or_released(location_hash, pred)
            || self
                .specfence
                .sf_tips
                .live_writer(location_hash)
                .is_some_and(|w| w == pred);
        // Thin: cheap scheduler-first spin; tip/Data probe every 64 iters.
        // Soft=0 forbids Blocking park / Estimate Block / Rewind.
        if thin {
            if !executing && !has_sf_tip {
                return Ok(());
            }
            self.specfence.sf_tips.record_wait_once_consume();
            if executing {
                const SPIN: usize = 4_096;
                for i in 0..SPIN {
                    if self.specfence.scheduler.is_done(pred)
                        || self.specfence.scheduler.is_validated(pred)
                    {
                        self.specfence.sf_tips.record_avoid_publish();
                        self.specfence.sf_tips.record_class_avoid(class);
                        return Ok(());
                    }
                    if i % 64 == 0 && sf.true_publish_ready(location_hash, pred) {
                        self.specfence.sf_tips.record_avoid_publish();
                        self.specfence.sf_tips.record_class_avoid(class);
                        return Ok(());
                    }
                    if !self.specfence.scheduler.is_executing(pred) {
                        break;
                    }
                    std::hint::spin_loop();
                }
            }
            if sf.true_publish_ready(location_hash, pred)
                || self.specfence.scheduler.is_done(pred)
                || self.specfence.scheduler.is_validated(pred)
            {
                self.specfence.sf_tips.record_avoid_publish();
                self.specfence.sf_tips.record_class_avoid(class);
                return Ok(());
            }
            // Soft=0 thin: no Blocking park; Opt-fallthrough (Learn raises Avoid).
            // Do not register_waiter — ungated finish never wakes DashMap waiters.
            return Ok(());
        }
        // Large: park when Executing / SF tip / live_writer. Estimate tip is
        // OCC residue used only as a *liveness* hint after abort cleared the
        // SF tip — Blocking still goes through park_publish_wait so
        // estimate_block_sf stays 0 (SoT: no Estimate Block on SF path).
        // ChainSpineTip: prefer short Released-spin before Blocking park so
        // true publish Avoid does not serialize the sticky spine.
        let live = executing
            || has_sf_tip
            || matches!(
                self.mv_memory.entry_kind_at(location_hash, pred),
                "estimate"
            );
        if !live {
            // Protected ℓ: do not Opt-fall into another FullReplay. Park on
            // the ordered pred until the true tip is published (scheduler
            // dependency, not an Estimate mutex and not a core-pinning spin).
            let unpublished = protected
                && !finished
                && !sf.true_publish_ready(location_hash, pred);
            if !unpublished {
                return Ok(());
            }
        }
        self.specfence.sf_tips.record_wait_once_consume();
        // ChainSpineTip: brief Released poll (not long busy-spin) before park.
        if self.specfence.sf_tips.is_chain_loc(location_hash) && (executing || has_sf_tip) {
            const SPIN: usize = 512;
            for i in 0..SPIN {
                if self.specfence.scheduler.is_done(pred)
                    || self.specfence.scheduler.is_validated(pred)
                    || (i % 16 == 0 && sf.true_publish_ready(location_hash, pred))
                {
                    self.specfence.sf_tips.record_avoid_publish();
                    self.specfence.sf_tips.record_class_avoid(class);
                    return Ok(());
                }
                if !self.specfence.scheduler.is_executing(pred) {
                    break;
                }
                std::hint::spin_loop();
            }
            if sf.true_publish_ready(location_hash, pred)
                || self.specfence.scheduler.is_done(pred)
                || self.specfence.scheduler.is_validated(pred)
            {
                self.specfence.sf_tips.record_avoid_publish();
                self.specfence.sf_tips.record_class_avoid(class);
                return Ok(());
            }
            // Prefer schedule one-hop: plant waiters wake on chain_release →
            // Q_released. Exact waiter covers opportunistic Version readers.
            // Soft=0 try_execute_sf skips Aborting for chain (mark_wait only).
            self.specfence
                .sf_tips
                .register_waiter(location_hash, pred, self.tx_idx);
            self.specfence
                .ready_edges
                .note_ungated_wait_on(self.tx_idx, pred);
            return Err(self.park_publish_wait(location_hash, pred));
        }
        match live_writer_act(
            &self.specfence,
            self.tx_idx,
            self.is_lazy,
            address,
            location_hash,
            pred,
        ) {
            crate::specfence::LiveAct::Block => {
                self.specfence
                    .sf_tips
                    .register_waiter(location_hash, pred, self.tx_idx);
                Err(self.park_publish_wait(location_hash, pred))
            }
            crate::specfence::LiveAct::Retry => Err(ReadError::InconsistentRead),
            crate::specfence::LiveAct::Skip => Ok(()),
        }
    }

    /// Mode dispatch. OCC is `Ok(())` with **zero** SpecFence calls.
    /// SpecFence OptimisticRead compiles to the same OCC proceed (`occ_optimistic_read`).
    fn maybe_wait(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        is_program: bool,
    ) -> Result<(), ReadError> {
        match self.specfence.mode {
            crate::ConcurrencyMode::Occ => Ok(()),
            crate::ConcurrencyMode::Pcc => self.maybe_wait_pcc(address, location_hash),
            crate::ConcurrencyMode::SpecFence if self.optimistic_skip_gate || self.is_lazy => {
                Ok(())
            }
            crate::ConcurrencyMode::SpecFence => {
                self.specfence_access_gate(address, location_hash, is_program)
            }
        }
    }

    /// Legacy PCC sticky Wait (not SpecFence π).
    fn maybe_wait_pcc(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
    ) -> Result<(), ReadError> {
        if !self
            .specfence
            .should_wait_location(&self.mv_memory.regions, location_hash, &address)
        {
            return Ok(());
        }
        if let Some(prev) = self
            .mv_memory
            .last_writer_before(location_hash, self.tx_idx)
            && !self.specfence.scheduler.is_done(prev)
        {
            self.specfence.metrics.record_wait(address);
            return Err(ReadError::Blocking(prev));
        }
        if let Some(prev) =
            self.specfence
                .wait_blocker(&self.mv_memory.regions, &address, self.tx_idx)
        {
            self.specfence.metrics.record_wait(address);
            return Err(ReadError::Blocking(prev));
        }
        Ok(())
    }

    /// SpecFence access gate. Mode(a) from \(a + e_{\mathrm{vis}} + \mathrm{PE}\).
    /// Quiet + empty PE ⇒ byte-identical OCC (`Ok(())`, no detect / ordinal).
    /// PE-on: live true-\(k\) on the access stream; decide only for PE \(\ell\).
    /// Fence certs only after a successful verb. Never `pcc_armed` rem overlay.
    fn specfence_access_gate(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        is_program: bool,
    ) -> Result<(), ReadError> {
        self.pcc_armed.set(false);
        if address == self.specfence.beneficiary || self.is_lazy {
            return Ok(());
        }
        // Thin-shell A0: same cost class as occ_optimistic_read even after aborts seed PE.
        if self
            .specfence
            .policy
            .is_some_and(|p| p.skip_ungated_tx_path_tax())
            && !self.specfence.ready_edges.is_gated(self.tx_idx)
        {
            return Ok(());
        }
        // Same-spine A0: empty PE → OptimisticRead, no Fence meta.
        // Not a second OCC runtime (v9.3).
        if crate::specfence::specfence_cost_class_spec(
            crate::ConcurrencyMode::SpecFence,
            self.specfence.learner,
        ) {
            return Ok(());
        }
        // basic/storage already recorded this k. Do not note again.
        let access_k = self
            .specfence
            .access_log
            .first_k(self.tx_idx, location_hash)
            .unwrap_or(0);
        if self.specfence.access_arms.is_never(location_hash) {
            return Ok(());
        }
        if !self.specfence.learner.location_predicted(location_hash) {
            // Do **not** clone Basic(addr) PE onto every Storage(addr,slot).
            // That opened WaitFor/ESTIMATE on the first SLOAD of a hint-fan
            // account (14689597: 263 wait→full_abort vs OCC 29). Storage RAW
            // becomes PE-on at abort true-k / InterPrior / Bayes admit only.
            return Ok(());
        }

        let vis = self.access_vis(location_hash);
        let bayes_q = self.specfence.bayes.query_access(
            location_hash,
            vis.writer_executing,
            vis.unfinished,
            vis.published_data,
        );
        match crate::specfence::decide_access_queried(
            self.specfence.learner,
            location_hash,
            access_k,
            Some(&vis),
            Some(bayes_q),
        ) {
            AccessDecision::OptimisticReadOcc { .. } => Ok(()),
            AccessDecision::OrderedAdmit => {
                if self
                    .mv_memory
                    .last_data_before(location_hash, self.tx_idx)
                    .is_none()
                {
                    self.specfence.metrics.record_pcc_roi_skip();
                    return Ok(());
                }
                self.note_fence_success(location_hash);
                self.specfence.metrics.record_predicted_essential();
                self.specfence.metrics.record_pcc_fire_at_a();
                self.specfence.metrics.record_edge_ordered_admit();
                self.specfence
                    .learner
                    .note_ordered_admit_success(location_hash);
                self.record_fence_process(
                    location_hash,
                    access_k,
                    is_program,
                    &vis,
                    ProcessReason::OrderedAdmitPublished,
                    DecisionVerb::OrderedAdmit,
                );
                let _ = address;
                Ok(())
            }
            AccessDecision::WaitFor { writer } => {
                self.specfence.metrics.record_predicted_essential();
                self.pcc_wait_for_writer(address, location_hash, access_k, is_program, writer)
            }
            AccessDecision::SerialLane { writer } => {
                self.specfence.metrics.record_predicted_essential();
                self.pcc_serial_lane(address, location_hash, access_k, is_program, writer)
            }
        }
    }

    /// \(e_{\mathrm{vis}}\) + learning features. Gathered only on a PE hit.
    /// `unfinished` is **!done only** (v6 S2).
    fn access_vis(&self, location_hash: MemoryLocationHash) -> AccessVis {
        let published_data = self
            .mv_memory
            .last_data_before(location_hash, self.tx_idx)
            .is_some();
        let is_done = |t: TxIdx| self.specfence.scheduler.is_done(t);
        let sketch =
            self.specfence
                .sketch
                .unfinished_writers_before(location_hash, self.tx_idx, is_done);
        let (writer, unfinished) = crate::specfence::compose_unfinished(
            sketch,
            self.mv_memory
                .last_writer_before(location_hash, self.tx_idx),
            self.mv_memory
                .residual_writer_before(location_hash, self.tx_idx),
            self.specfence
                .partial_retry
                .force_writer(self.tx_idx, location_hash),
            self.tx_idx,
            is_done,
        );
        let writer_executing = writer.is_some_and(|w| self.specfence.scheduler.is_executing(w));
        let hot = self.specfence.hotset.contains(location_hash)
            || self.specfence.hotset.writer_count(location_hash) >= 2
            || self.specfence.learner.writer_count_live(location_hash) >= 2
            || ((self.specfence.rw_prior.predicts_write(location_hash)
                || self.specfence.rw_prior.write_confidence(location_hash) > 0.25)
                && unfinished > 0);
        let ws_hat = self.specfence.rw_prior.predicts_write(location_hash)
            || self.specfence.rw_prior.write_confidence(location_hash) > 0.25;
        if hot || ws_hat {
            self.specfence
                .learner
                .note_hot_ws_posterior(location_hash, true);
            // Refresh only — first ReadyEdge insert is admit_seed (begin_block / abort).
            if self
                .specfence
                .ready_edges
                .predicted_producer(location_hash)
                .is_some()
                && let Some(w) = writer
                && unfinished > 0
                && w < self.tx_idx
            {
                self.specfence
                    .ready_edges
                    .note_raw_producer(location_hash, w);
                self.specfence.ready_edges.note_consumer(self.tx_idx, w);
                self.specfence.producer_stages.reserve(w);
                self.specfence.scheduler.admit_spine(w, self.specfence.wave);
            }
        }
        let predicted = self.specfence.ready_edges.predicted_producer(location_hash);
        AccessVis {
            published_data,
            writer,
            writer_executing,
            unfinished,
            in_serial_lane: self.specfence.sketch.in_serial_lane(location_hash),
            hot,
            ws_hat,
            independence_certified: self.specfence.sketch.independence_certified(location_hash)
                && unfinished == 0,
            tip_is_conflict_producer: writer.is_some() && writer == predicted,
        }
    }

    /// Telemetry for a successful Fence verb (CC process plane).
    fn record_fence_process(
        &self,
        location_hash: MemoryLocationHash,
        access_k: u32,
        is_program: bool,
        vis: &AccessVis,
        reason: ProcessReason,
        verb: DecisionVerb,
    ) {
        self.specfence
            .process
            .record(location_hash, self.tx_idx, reason, true, false);
        self.specfence.process.record_decision(DecisionFeat {
            verb,
            access_k,
            depth: 0,
            incarnation: self.tx_incarnation,
            is_program,
            writer_published: vis.published_data,
            writer_validated: vis
                .writer
                .is_some_and(|w| self.specfence.scheduler.is_validated(w)),
            writer_executing: vis.writer_executing,
            writer_ready: vis
                .writer
                .is_some_and(|w| self.specfence.scheduler.is_ready(w)),
            writer_present: vis.writer.is_some(),
            avoid_broadcast: false,
            canary_ok: false,
            independence_certified: vis.independence_certified,
            essential_antidep: true,
            force_prefix: false,
            clique_gated: false,
            in_hot_set: vis.hot,
            prior_warm: vis.ws_hat,
            mode_read: true,
        });
    }

    /// Successful Fence verb — strip + rem-legal mirror. Never call on Data miss.
    fn note_fence_success(&self, location: MemoryLocationHash) {
        let first = !self.specfence.certificates.has_any(self.tx_idx);
        self.specfence
            .certificates
            .note_success(self.tx_idx, location);
        // Certificate strip is the rem-legal SoT (kernel merged).
        if first {
            self.specfence.metrics.record_pcc_kernel_exec();
        }
    }

    /// SerialLane = exclusive progress token. Never prefer_admit + Spec continue.
    fn pcc_serial_lane(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        access_k: u32,
        is_program: bool,
        w: TxIdx,
    ) -> Result<(), ReadError> {
        self.specfence
            .sketch
            .mark_access_class(location_hash, access_k);
        self.specfence.lanes.grant(location_hash, access_k, w);
        self.specfence
            .ready_edges
            .note_raw_producer(location_hash, w);
        self.specfence.ready_edges.note_consumer(self.tx_idx, w);
        self.specfence.producer_stages.reserve(w);
        self.specfence.scheduler.admit_spine(w, self.specfence.wave);
        match crate::specfence::ordered_admit_act::act_serial_lane(self.specfence.scheduler, w) {
            crate::specfence::ordered_admit_act::OrderedAdmitAct::DoneOptimisticRead { cert } => {
                if cert {
                    self.note_fence_success(location_hash);
                    self.specfence.metrics.record_ordered_admit_after_done();
                }
                return self.occ_optimistic_read();
            }
            crate::specfence::ordered_admit_act::OrderedAdmitAct::WaitForDependency { writer } => {
                return self.pcc_wait_for_writer(
                    address,
                    location_hash,
                    access_k,
                    is_program,
                    writer,
                );
            }
            crate::specfence::ordered_admit_act::OrderedAdmitAct::ReadyCanary => {}
        }
        // Ready/Validated/Aborting: one Spec canary + ProducerStage/edge so
        // the next incarnation is refused while w is Executing. Parking or
        // ready-refuse of a Ready head yield-spins (iter20 / v6 hang class).
        // Not prefer_admit-without-progress: w is reserved and admitted.
        if self.specfence.scheduler.is_ready(w) || self.specfence.scheduler.is_validated(w) {
            self.specfence.process.record(
                location_hash,
                self.tx_idx,
                ProcessReason::WaitForSerial,
                true,
                false,
            );
        }
        self.occ_optimistic_read()
    }

    /// ESTIMATE observe only. Must **not** mark PE — that opens decide/ordinal
    /// for the rest of the first wave and OrderedAdmit-theaters stale Data
    /// (14689597: 581 SF aborts vs OCC 24). Abort still trains true-k PE.
    /// First ReadyEdge insert is admit_seed; refresh only when the producer
    /// was already predicted.
    fn note_unpublished_raw(&self, location: MemoryLocationHash, writer: TxIdx) {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence {
            return;
        }
        self.specfence.sketch.push_spine(location, writer);
        self.specfence
            .ready_edges
            .note_unpublished(location, writer);
        if self
            .specfence
            .ready_edges
            .predicted_producer(location)
            .is_some()
            && writer < self.tx_idx
        {
            self.specfence
                .ready_edges
                .note_consumer(self.tx_idx, writer);
            self.specfence.producer_stages.reserve(writer);
        }
        let hot_or_ws = self.specfence.hotset.contains(location)
            || self.specfence.rw_prior.predicts_write(location);
        self.specfence
            .learner
            .note_hot_ws_posterior(location, hot_or_ws);
    }

    /// ESTIMATE / aborted-incarnation Blocking: PE-known RAW → WaitForDependency;
    /// unknown ESTIMATE stays BlockingOther (true OCC).
    /// SpecFence Soft=0 must not land here — use [`Self::park_publish_wait`].
    fn park_estimate_blocking(
        &self,
        location_hash: MemoryLocationHash,
        writer: TxIdx,
    ) -> ReadError {
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.specfence.sf_tips.record_estimate_block_sf();
        }
        // leftover_min must not ghost-park on a done / later writer.
        // InconsistentRead → Retry so the incarnation re-reads Data.
        if self.specfence.ready_edges.is_live_leftover_min(self.tx_idx)
            && (writer > self.tx_idx
                || self.specfence.scheduler.is_done(writer)
                || self.specfence.scheduler.is_validated(writer)
                || self
                    .specfence
                    .ready_edges
                    .leftover_min_skips_blocker(writer))
        {
            return ReadError::InconsistentRead;
        }
        let pe_known = self.specfence.learner.location_predicted(location_hash)
            || self
                .specfence
                .ready_edges
                .predicted_producer(location_hash)
                .is_some()
            || self.specfence.access_arms.is_wait_once(location_hash);
        let mut kind = crate::specfence::ordered_admit_act::estimate_park_kind(pe_known);
        let access_k = self
            .specfence
            .access_log
            .first_k(self.tx_idx, location_hash)
            .unwrap_or_else(|| self.specfence.partial_retry.current_k(self.tx_idx) as u32);
        let armed_at_k = if kind == crate::specfence::ParkKind::WaitForDependency {
            let prefix = self
                .specfence
                .access_log
                .prefix_before(self.tx_idx, access_k);
            let armed = self
                .specfence
                .partial_retry
                .arm_wait_for_dependency_checkpoint(
                    self.tx_idx,
                    location_hash,
                    access_k.max(1),
                    &prefix,
                );
            if crate::specfence::ordered_admit_act::wait_for_resume_armed(armed) {
                armed
            } else {
                // No honest prefix — WaitForDependency park wakes FullAbortReexecute
                // (14689597: 263 wait + 243 abort vs OCC 29). Stay OCC BlockingOther.
                kind = crate::specfence::ParkKind::BlockingOther;
                self.specfence.partial_retry.current_k(self.tx_idx) as u64
            }
        } else {
            self.specfence.partial_retry.current_k(self.tx_idx) as u64
        };
        self.specfence
            .wave
            .set_pending_park(location_hash, armed_at_k, kind);
        ReadError::Blocking(writer)
    }

    /// SpecFence WaitOnce publish-wait: park until true Data, not Estimate.
    /// Does **not** increment `estimate_block_sf`.
    fn park_publish_wait(
        &self,
        location_hash: MemoryLocationHash,
        writer: TxIdx,
    ) -> ReadError {
        if self.specfence.ready_edges.is_live_leftover_min(self.tx_idx)
            && (writer > self.tx_idx
                || self.specfence.scheduler.is_done(writer)
                || self.specfence.scheduler.is_validated(writer)
                || self
                    .specfence
                    .ready_edges
                    .leftover_min_skips_blocker(writer))
        {
            return ReadError::InconsistentRead;
        }
        let access_k = self
            .specfence
            .access_log
            .first_k(self.tx_idx, location_hash)
            .unwrap_or_else(|| self.specfence.partial_retry.current_k(self.tx_idx) as u32);
        let prefix = self
            .specfence
            .access_log
            .prefix_before(self.tx_idx, access_k);
        let armed = self
            .specfence
            .partial_retry
            .arm_wait_for_dependency_checkpoint(
                self.tx_idx,
                location_hash,
                access_k.max(1),
                &prefix,
            );
        let (kind, armed_at_k) =
            if crate::specfence::ordered_admit_act::wait_for_resume_armed(armed) {
                (crate::specfence::ParkKind::WaitForDependency, armed)
            } else {
                (
                    crate::specfence::ParkKind::BlockingOther,
                    self.specfence.partial_retry.current_k(self.tx_idx) as u64,
                )
            };
        self.specfence
            .wave
            .set_pending_park(location_hash, armed_at_k, kind);
        ReadError::Blocking(writer)
    }

    /// SF → publish-wait (no Estimate Block counter); OCC → estimate park.
    #[inline]
    fn park_live_writer(
        &self,
        location_hash: MemoryLocationHash,
        writer: TxIdx,
    ) -> ReadError {
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.park_publish_wait(location_hash, writer)
        } else {
            self.park_estimate_blocking(location_hash, writer)
        }
    }

    /// Shared OCC proceed: no rem journal / first_k / Edge / process / Detect DashMap.
    /// PrefixSkip / FF-head resume is Resolve (not OptimisticRead) — `pcc_armed` stays
    /// false so first-incarnation ¬PE still uses OCC ESTIMATE→Blocking.
    #[inline]
    fn occ_optimistic_read(&self) -> Result<(), ReadError> {
        self.pcc_armed.set(false);
        self.optimistic_read_this_tx
            .set(self.optimistic_read_this_tx.get().saturating_add(1));
        self.specfence.metrics.record_optimistic_read_occ_fast();
        self.specfence.metrics.record_edge_optimistic_read();
        self.specfence.metrics.record_optimistic_read();
        Ok(())
    }

    /// Resolve-read overlay: PCC Fire **or** PrefixSkip/FF resume (PartialAbortRewind).
    /// First-incarnation OptimisticRead stays OCC (no FF / OrderedDirtyRead).
    /// Edged SpecFence vis skips ESTIMATE tips via [`crate::specfence::sf_mv`].
    #[inline]
    fn resolve_read_overlay(&self) -> bool {
        self.pcc_armed.get()
            || self.specfence.partial_retry.is_rewind_resume(self.tx_idx)
            || self.specfence.partial_retry.has_ff_head(self.tx_idx)
            || self.sf_skip_estimate()
    }

    /// WaitReleased / OrderedTip: never consume the OCC race Estimate tip.
    #[inline]
    fn sf_skip_estimate(&self) -> bool {
        self.specfence.mode == crate::ConcurrencyMode::SpecFence && self.vis.needs_fence()
    }

    #[inline]
    fn note_sf_mv_read(&self) {
        if self.sf_skip_estimate() {
            self.specfence.metrics.record_sf_mv_read(self.vis);
        }
    }

    #[allow(dead_code)]
    fn pcc_ordered_admit_published(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        access_k: u32,
        is_program: bool,
        v: TxVersion,
    ) -> Result<(), ReadError> {
        self.pcc_armed.set(true);
        self.pcc_this_tx
            .set(self.pcc_this_tx.get().saturating_add(1));
        self.specfence.metrics.record_pcc_fire_at_a();
        self.specfence.metrics.record_edge_ordered_admit();
        self.specfence
            .learner
            .note_ordered_admit_success(location_hash);
        let access_depth = 0u8;
        let key = EdgeKey {
            location: location_hash,
            reader: self.tx_idx,
            access_k,
            depth: access_depth,
        };
        self.specfence.edges.record(
            key,
            Some(v.tx_idx),
            EdgeKind::Wr,
            if self.specfence.scheduler.is_validated(v.tx_idx) {
                EdgeState::Validated
            } else {
                EdgeState::PublishedUncommitted
            },
        );
        self.specfence.process.record(
            location_hash,
            self.tx_idx,
            ProcessReason::OrderedAdmitPublished,
            true,
            false,
        );
        self.specfence.process.record_decision(DecisionFeat {
            verb: DecisionVerb::OrderedAdmit,
            access_k,
            depth: access_depth,
            incarnation: self.tx_incarnation,
            is_program,
            writer_published: true,
            writer_validated: self.specfence.scheduler.is_validated(v.tx_idx),
            writer_executing: false,
            writer_ready: false,
            writer_present: true,
            avoid_broadcast: false,
            canary_ok: false,
            independence_certified: false,
            essential_antidep: true,
            force_prefix: false,
            clique_gated: false,
            in_hot_set: false,
            prior_warm: false,
            mode_read: true,
        });
        self.specfence
            .sketch
            .install_data_residual(location_hash, v.tx_idx, v.tx_incarnation);
        self.ordered_admit_on_data_lite(address, location_hash, v, false)
    }

    fn pcc_wait_for_writer(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        access_k: u32,
        is_program: bool,
        w: TxIdx,
    ) -> Result<(), ReadError> {
        // WaitForDependency one producer. Never `pcc_armed` — rem overlay skips ESTIMATE
        // and OrderedAdmit-theaters stale last_data (14689597 abort 503 vs OCC 113).
        let _ = (address, is_program);
        self.specfence
            .ready_edges
            .note_raw_producer(location_hash, w);
        self.specfence.ready_edges.note_consumer(self.tx_idx, w);
        self.specfence.producer_stages.reserve(w);
        self.specfence.scheduler.admit_spine(w, self.specfence.wave);
        match crate::specfence::ordered_admit_act::act_wait_for(self.specfence.scheduler, w) {
            crate::specfence::ordered_admit_act::OrderedAdmitAct::DoneOptimisticRead { cert } => {
                // OrderedAdmit-after-Done is tax. DoneOptimisticRead cert is partial_abort bait
                // (sibling optimistic_read stays uncertified → attempt then full_abort_reexecute).
                if cert {
                    self.note_fence_success(location_hash);
                    self.specfence.metrics.record_ordered_admit_after_done();
                }
                return self.occ_optimistic_read();
            }
            crate::specfence::ordered_admit_act::OrderedAdmitAct::ReadyCanary => {
                return self.occ_optimistic_read();
            }
            crate::specfence::ordered_admit_act::OrderedAdmitAct::WaitForDependency {
                writer: _,
            } => {}
        }
        // WaitOnce: the same (tx, ℓ, w) does not enter the park body again.
        match live_writer_act(
            &self.specfence,
            self.tx_idx,
            self.is_lazy,
            address,
            location_hash,
            w,
        ) {
            crate::specfence::LiveAct::Block => {}
            crate::specfence::LiveAct::Retry => return Err(ReadError::InconsistentRead),
            crate::specfence::LiveAct::Skip => return self.occ_optimistic_read(),
        }
        let prefix = self
            .specfence
            .access_log
            .prefix_before(self.tx_idx, access_k);
        let armed_at_k = self
            .specfence
            .partial_retry
            .arm_wait_for_dependency_checkpoint(self.tx_idx, location_hash, access_k, &prefix);
        // Edges already reserved. Park only when rem can ResumeAtK.
        // Park-then-full_abort_reexecute is idle + OCC abort (14689597).
        if !crate::specfence::ordered_admit_act::wait_for_resume_armed(armed_at_k) {
            return self.occ_optimistic_read();
        }
        self.note_fence_success(location_hash);
        self.pcc_this_tx
            .set(self.pcc_this_tx.get().saturating_add(1));
        self.specfence.metrics.record_pcc_fire_at_a();
        self.specfence.metrics.record_edge_wait_for();
        self.specfence.metrics.record_wait_hard();
        self.specfence.process.record(
            location_hash,
            self.tx_idx,
            ProcessReason::WaitForWriter,
            true,
            false,
        );
        self.specfence.process.record_decision(DecisionFeat {
            verb: DecisionVerb::WaitFor,
            access_k,
            depth: 0,
            incarnation: self.tx_incarnation,
            is_program,
            writer_published: false,
            writer_validated: false,
            writer_executing: true,
            writer_ready: false,
            writer_present: true,
            avoid_broadcast: false,
            canary_ok: false,
            independence_certified: false,
            essential_antidep: true,
            force_prefix: false,
            clique_gated: false,
            in_hot_set: false,
            prior_warm: false,
            mode_read: true,
        });
        let key = EdgeKey {
            location: location_hash,
            reader: self.tx_idx,
            access_k,
            depth: 0,
        };
        self.specfence
            .edges
            .record(key, Some(w), EdgeKind::Wr, EdgeState::Unpublished);
        self.specfence.learner.note_park_heat();
        self.specfence
            .dag
            .arm_hard_wait(location_hash, self.tx_idx, w);
        self.specfence.wave.set_pending_park(
            location_hash,
            armed_at_k,
            crate::specfence::ParkKind::WaitForDependency,
        );
        self.specfence.process.note_park(self.tx_idx);
        Err(ReadError::Blocking(w))
    }

    /// Legacy rem WaitFor museum — **not** the SpecFence product path.
    /// Product Avoid is `pcc_wait_for_writer` → `ordered_admit_act::act_wait_for`
    /// (WaitForDependency / DoneOptimisticRead). Kept for inspect/lab residual OrderedAdmit SoT.
    /// SoftWait Soft stays 0.
    #[allow(dead_code)]
    fn fence_wait_for(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        requested: TxIdx,
        avoid: bool,
        in_h: bool,
        clique_gated: bool,
        canary_taken: bool,
        force_prefix: bool,
        must_wait: bool,
    ) -> Result<(), ReadError> {
        // U1: protocol TLS must not optimistic_read a force_prefix / must_wait Region.
        if crate::specfence::protocol_tls_active() && !must_wait {
            self.specfence.process.record(
                location_hash,
                self.tx_idx,
                ProcessReason::OptimisticReadProtocolTls,
                avoid,
                canary_taken,
            );
            self.specfence.metrics.record_edge_optimistic_read();
            self.specfence.metrics.record_optimistic_read();
            note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
            return Ok(());
        }

        // Data may have landed between π and park.
        if let Some((tx_idx, tx_incarnation)) =
            self.mv_memory.last_data_before(location_hash, self.tx_idx)
        {
            self.specfence
                .sketch
                .install_data_residual(location_hash, tx_idx, tx_incarnation);
            self.specfence.metrics.record_edge_ordered_admit();
            self.specfence
                .learner
                .note_ordered_admit_success(location_hash);
            self.specfence.process.record(
                location_hash,
                self.tx_idx,
                ProcessReason::OrderedAdmitPublished,
                avoid,
                canary_taken,
            );
            return self.ordered_admit_on_data_lite(
                address,
                location_hash,
                TxVersion {
                    tx_idx,
                    tx_incarnation,
                },
                force_prefix,
            );
        }

        let is_done = |t: TxIdx| self.specfence.scheduler.is_done(t);
        let is_executing = |t: TxIdx| self.specfence.scheduler.is_executing(t);
        let unfinished_all =
            self.specfence
                .sketch
                .unfinished_writers_before(location_hash, self.tx_idx, is_done);
        // Hang-freedom: admit the WaitFor target only. Fleet PreferAdmit of
        // unfinished_all / ready_spines parks independents (6196166).
        // Park Executing first. Ready is prefer-admitted (not parked).
        // Done∅Data → OrderedAdmit residual — never OptimisticReadWriterDone on must_wait.
        let unfinished_exec = unfinished_all
            .iter()
            .copied()
            .rev()
            .find(|&w| is_executing(w));
        let requested_exec =
            (requested < self.tx_idx && is_executing(requested)).then_some(requested);
        let requested_live = (requested < self.tx_idx && !is_done(requested)).then_some(requested);

        let (target, reason) = if let Some(w) = unfinished_exec.or(requested_exec) {
            (Some(w), ProcessReason::WaitForWriter)
        } else if must_wait {
            // U3: park only Executing. Ready is prefer-admitted; Done∅Data
            // → OrderedAdmit residual. force_prefix must not WaitFor(reader-1)
            // (exclude-set / wait_no_writer smell).
            (None, ProcessReason::OrderedAdmitPublished)
        } else if let Some(w) = requested_live {
            let r = if w + 1 == self.tx_idx {
                ProcessReason::WaitForSerial
            } else {
                ProcessReason::WaitForWriter
            };
            (Some(w), r)
        } else {
            (None, ProcessReason::OptimisticReadWriterDone)
        };

        if let Some(t) = target {
            if t >= self.tx_idx {
                self.specfence.process.record(
                    location_hash,
                    self.tx_idx,
                    ProcessReason::OptimisticReadInversion,
                    avoid,
                    canary_taken,
                );
                self.specfence.metrics.record_edge_optimistic_read();
                self.specfence.metrics.record_optimistic_read();
                note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
                return Ok(());
            }
            if clique_gated || in_h || self.specfence.sketch.in_serial_lane(location_hash) {
                self.specfence.metrics.record_spine_wait();
            }
            self.specfence
                .process
                .record(location_hash, self.tx_idx, reason, avoid, canary_taken);
            self.specfence.learner.note_park_heat();
            self.specfence.metrics.record_edge_wait_for();
            self.specfence.metrics.record_wait_hard();
            self.specfence.metrics.record_wait(address);
            self.specfence
                .dag
                .arm_hard_wait(location_hash, self.tx_idx, t);
            self.specfence.scheduler.admit_spine(t, self.specfence.wave);
            let armed_at_k = self.specfence.partial_retry.current_k(self.tx_idx) as u64;
            let pe_known = self.specfence.learner.location_predicted(location_hash)
                || self
                    .specfence
                    .ready_edges
                    .predicted_producer(location_hash)
                    .is_some();
            self.specfence.wave.set_pending_park(
                location_hash,
                armed_at_k,
                crate::specfence::ordered_admit_act::estimate_park_kind(pe_known),
            );
            self.specfence.process.note_park(self.tx_idx);
            return Err(ReadError::Blocking(t));
        }

        // No live wait target and no Data. Avoid/essential: OrderedAdmit residual
        // (Done→Data SoT). OptimisticReadWriterDone is not a hang-freedom escape.
        self.specfence
            .learner
            .note_writer_done(location_hash, avoid);
        self.specfence.sketch.note_hot(location_hash);
        self.specfence.metrics.record_writer_done_learned();

        if must_wait {
            // Writer is not Executing (that path parked). Recheck Data once;
            // otherwise residual SoT (last Data or Storage). No hope-spin.
            if let Some((tx_idx, tx_incarnation)) =
                self.mv_memory.last_data_before(location_hash, self.tx_idx)
            {
                self.specfence
                    .sketch
                    .install_data_residual(location_hash, tx_idx, tx_incarnation);
                return self.ordered_admit_residual_data(
                    address,
                    location_hash,
                    TxVersion {
                        tx_idx,
                        tx_incarnation,
                    },
                    force_prefix,
                    avoid,
                    canary_taken,
                );
            }
            return self.ordered_admit_done_residual(
                address,
                location_hash,
                requested,
                force_prefix,
                avoid,
                canary_taken,
            );
        }

        // Cold / non-Fence: storage-origin OptimisticRead is allowed.
        // force_prefix is exclude-set — do not count as a live OptimisticRead leak.
        let leak = if avoid {
            ProcessReason::OptimisticReadAfterAvoid
        } else {
            ProcessReason::OptimisticReadWriterDone
        };
        self.specfence
            .process
            .record(location_hash, self.tx_idx, leak, avoid, canary_taken);
        self.specfence.metrics.record_edge_optimistic_read();
        self.specfence.metrics.record_optimistic_read();
        note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
        Ok(())
    }

    /// OrderedAdmit last committed / residual Data after writer Done∅Data.
    fn ordered_admit_residual_data(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        v: TxVersion,
        force_prefix: bool,
        avoid: bool,
        canary_taken: bool,
    ) -> Result<(), ReadError> {
        self.specfence.metrics.record_edge_ordered_admit();
        self.specfence.metrics.record_ordered_admit_residual();
        self.specfence
            .learner
            .note_ordered_admit_success(location_hash);
        self.specfence.process.record(
            location_hash,
            self.tx_idx,
            ProcessReason::OrderedAdmitPublished,
            avoid,
            canary_taken,
        );
        self.ordered_admit_on_data_lite(address, location_hash, v, force_prefix)
    }

    /// Region residual OrderedAdmit: last Data residual, else Storage residual.
    fn ordered_admit_done_residual(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        requested: TxIdx,
        force_prefix: bool,
        avoid: bool,
        canary_taken: bool,
    ) -> Result<(), ReadError> {
        if let Some((tx_idx, tx_incarnation)) =
            self.mv_memory.last_data_before(location_hash, self.tx_idx)
        {
            self.specfence
                .sketch
                .install_data_residual(location_hash, tx_idx, tx_incarnation);
            return self.ordered_admit_residual_data(
                address,
                location_hash,
                TxVersion {
                    tx_idx,
                    tx_incarnation,
                },
                force_prefix,
                avoid,
                canary_taken,
            );
        }
        match self
            .specfence
            .sketch
            .residual_ordered_admit(location_hash)
            .unwrap_or_else(|| {
                self.specfence.sketch.install_done_residual(
                    location_hash,
                    requested.min(self.tx_idx.saturating_sub(1)),
                )
            }) {
            crate::specfence::ResidualOrderedAdmit::Data {
                tx_idx,
                tx_incarnation,
            } => self.ordered_admit_residual_data(
                address,
                location_hash,
                TxVersion {
                    tx_idx,
                    tx_incarnation,
                },
                force_prefix,
                avoid,
                canary_taken,
            ),
            crate::specfence::ResidualOrderedAdmit::Storage { .. } => {
                self.specfence.metrics.record_edge_ordered_admit();
                self.specfence.metrics.record_ordered_admit_residual();
                self.specfence
                    .learner
                    .note_ordered_admit_success(location_hash);
                self.specfence.process.record(
                    location_hash,
                    self.tx_idx,
                    ProcessReason::OrderedAdmitPublished,
                    avoid,
                    canary_taken,
                );
                self.specfence
                    .partial_retry
                    .note_access_certified_checkpoint(self.tx_idx, location_hash);
                self.specfence.rem.note_checkpoint_opportunity();
                crate::specfence::arm_pending_effect_cp_only();
                let _ = address;
                Ok(())
            }
        }
    }

    /// OrderedAdmit-on-Data: OCC-like certify of published MV Data without `is_done` park.
    /// Writer abort → ESTIMATE → SuffixRepair/RebindOnly. Repair Await uses
    /// BlockingOther (WaitHard→BO) — no FenceGraph SoftWait Soft.
    fn ordered_admit_on_data_lite(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
        v: TxVersion,
        force_prefix: bool,
    ) -> Result<(), ReadError> {
        let _ = address;
        self.specfence.metrics.record_ordered_admit_hit();
        if force_prefix || self.specfence.rw_prior.predicts_write(location_hash) {
            self.specfence.metrics.record_prior_ordered_admit_hit();
        }
        self.specfence
            .partial_retry
            .note_access_certified_checkpoint(self.tx_idx, location_hash);
        self.specfence.rem.note_checkpoint_opportunity();
        crate::specfence::arm_pending_effect_cp_only();
        // Iter26: do NOT arm OrderedAdmit-snap on OrderedAdmit-on-Data — even Validated OrderedAdmit can
        // append tip_sloads slots absent from FF → refuse-if-stale. FF-path only
        // (try_ff_storage) keeps tip≡FF. Keep all-prefix Validated spin.
        let _ = v;
        Ok(())
    }

    /// P2 EarlyVal after an OptimisticRead origin is recorded: certify or abort early.
    fn maybe_early_val(
        &mut self,
        address: Address,
        location_hash: MemoryLocationHash,
    ) -> Result<(), ReadError> {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence {
            return Ok(());
        }
        // R0/R1: EarlyVal only on HotSet under research inspect (default path: off).
        if self.specfence.engagement.is_lean() || !self.specfence.hotset.contains(location_hash) {
            return Ok(());
        }
        if address == self.specfence.beneficiary {
            return Ok(());
        }
        let posterior = self
            .specfence
            .bayes
            .conflict_probability(location_hash, Some(&address));
        // Only EarlyVal when cheap/pressure: high P_conflict or hot OptimisticRead.
        if early_val_probability(posterior) < 0.35
            && self.mv_memory.regions.location_mode(location_hash)
                != crate::specfence::RegionMode::Wait
        {
            return Ok(());
        }
        let origins = match self.read_set.get(&location_hash) {
            Some(o) => o.clone(),
            None => return Ok(()),
        };
        if self
            .mv_memory
            .origins_still_valid(self.tx_idx, location_hash, &origins)
        {
            self.specfence
                .partial_retry
                .note_certified(self.tx_idx, location_hash);
            self.specfence.rem.note_checkpoint_opportunity();
            self.specfence.metrics.record_checkpoint_opportunity();
            // M1d: live Inspector snap via step_end when protocol TLS active.
            note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
            Ok(())
        } else {
            // EarlyVal fail → enter PartialRetry path (re-exec with force-ordered_admit).
            let mut certified = self
                .specfence
                .partial_retry
                .force_ordered_admit_locations(self.tx_idx);
            for loc in self.read_set.keys() {
                if *loc != location_hash
                    && !certified.contains(loc)
                    && self.mv_memory.origins_still_valid(
                        self.tx_idx,
                        *loc,
                        self.read_set.get(loc).unwrap(),
                    )
                {
                    certified.push(*loc);
                }
            }
            // M1/M1b: demote head PartialRetry → RewindTo + journal FF when a cp exists.
            let k_fail = self
                .specfence
                .partial_retry
                .first_k(self.tx_idx, location_hash)
                .unwrap_or_else(|| self.specfence.partial_retry.current_k(self.tx_idx));
            let cp = self
                .specfence
                .partial_retry
                .last_checkpoint_before(self.tx_idx, k_fail)
                .unwrap_or(crate::specfence::CheckpointId {
                    tx_idx: self.tx_idx,
                    incarnation: 0,
                    k: 0,
                });
            self.specfence.partial_retry.arm_rewind_to(
                self.tx_idx,
                cp,
                k_fail,
                certified.clone(),
                Vec::new(),
                Vec::new(),
            );
            self.specfence
                .partial_retry
                .set_force_ordered_admit(self.tx_idx, certified);
            self.specfence.metrics.record_partial_retry();
            self.specfence.metrics.record_rewind_to_cp();
            self.specfence.metrics.record_region_validate_fail(1);
            Err(ReadError::InconsistentRead)
        }
    }

    // Push a new read origin. Return an error when there's already
    // an origin but doesn't match the new one to force re-execution.
    fn push_origin(read_origins: &mut ReadOrigins, origin: ReadOrigin) -> Result<(), ReadError> {
        if let Some(prev_origin) = read_origins.last() {
            if prev_origin != &origin {
                return Err(ReadError::InconsistentRead);
            }
        } else {
            read_origins.push(origin);
        }
        Ok(())
    }

    /// Deep finegrain: count DB read + emit RawEffectEdge on cross-tx MV origin.
    fn deep_trace_read(
        &self,
        location: MemoryLocationHash,
        kind: crate::specfence::LocationKind,
        origin: Option<&ReadOrigin>,
    ) {
        let Some(fg) = self.specfence.finegrain else {
            return;
        };
        if !fg.deep_enabled() {
            return;
        }
        let producer = match origin {
            Some(ReadOrigin::MvMemory(v)) => Some((v.tx_idx, v.tx_incarnation)),
            _ => None,
        };
        fg.deep_note_db_read(
            self.tx_idx,
            self.tx_incarnation,
            producer,
            location,
            kind,
            true,
        );
    }

    fn get_code_hash(&mut self, address: Address) -> Result<Option<B256>, ReadError> {
        let location_hash = hash_deterministic(MemoryLocation::CodeHash(address));
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            let _ = self.specfence.access_log.note(self.tx_idx, location_hash);
        }
        let read_origins = self.read_set.entry(location_hash).or_default();

        // Try to read the latest code hash in [MvMemory]
        // TODO: Memoize read locations (expected to be small) here in [Vm] to avoid
        // contention in [MvMemory]
        let closest = {
            let _nest = crate::mv_memory::DataNest::enter("get_code_hash");
            self.mv_memory.data.get(&location_hash).and_then(|written| {
                written
                    .range(..self.tx_idx)
                    .next_back()
                    .map(|(idx, e)| (*idx, e.clone()))
            })
        };
        if let Some((tx_idx, MemoryEntry::Data(tx_incarnation, value))) = closest {
            if self
                .mv_memory
                .is_aborted_incarnation(tx_idx, tx_incarnation)
            {
                match live_writer_act(
                    &self.specfence,
                    self.tx_idx,
                    self.is_lazy,
                    address,
                    location_hash,
                    tx_idx,
                ) {
                    crate::specfence::LiveAct::Skip => {}
                    crate::specfence::LiveAct::Retry => {
                        return Err(ReadError::InconsistentRead);
                    }
                    crate::specfence::LiveAct::Block => {
                        return Err(ReadError::Blocking(tx_idx));
                    }
                }
            } else {
                match value {
                    MemoryValue::SelfDestructed => {
                        return Err(ReadError::SelfDestructedAccount);
                    }
                    MemoryValue::CodeHash(code_hash) => {
                        let origin = ReadOrigin::MvMemory(TxVersion {
                            tx_idx,
                            tx_incarnation,
                        });
                        Self::push_origin(read_origins, origin.clone())?;
                        self.deep_trace_read(
                            location_hash,
                            crate::specfence::LocationKind::CodeHash,
                            Some(&origin),
                        );
                        return Ok(Some(code_hash));
                    }
                    _ => {}
                }
            }
        }

        // Fallback to storage
        Self::push_origin(read_origins, ReadOrigin::Storage)?;
        self.deep_trace_read(
            location_hash,
            crate::specfence::LocationKind::CodeHash,
            Some(&ReadOrigin::Storage),
        );
        self.storage
            .code_hash(&address)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

/// Opt | WaitOnce | NeverWait. Not a method: callers hold `read_set`.
/// SpecFence Soft=0 thin: never Block on OCC Estimate alone — WaitOnce
/// consult owns thin consume. Large: AccessArm decide may Block once
/// (publish-wait, not Estimate counter) to preserve sticky/Rewind wins.
fn live_writer_act(
    specfence: &crate::specfence::SpecFenceCtx<'_>,
    tx_idx: TxIdx,
    is_lazy: bool,
    address: Address,
    location_hash: MemoryLocationHash,
    writer: TxIdx,
) -> crate::specfence::LiveAct {
    if specfence.mode != crate::ConcurrencyMode::SpecFence {
        return crate::specfence::LiveAct::Block;
    }
    if specfence.ready_edges.is_live_leftover_min(tx_idx)
        && address != specfence.beneficiary
        && !is_lazy
    {
        return crate::specfence::LiveAct::Block;
    }
    let never = address == specfence.beneficiary || is_lazy;
    let finished = specfence.scheduler.is_done(writer) || specfence.scheduler.is_validated(writer);
    let thin = specfence.scheduler.block_size() <= crate::specfence::THIN_SHELL_N;
    // Thin ungated WaitOnce: consult_ungated_wait_once owns consume.
    if !never
        && !finished
        && thin
        && !specfence.ready_edges.is_gated(tx_idx)
        && (specfence.access_arms.is_wait_once(location_hash)
            || specfence.access_arms.crit_loc_hash() == location_hash)
    {
        return crate::specfence::LiveAct::Skip;
    }
    // Thin non-WaitOnce: Skip Estimate Block (OCC-baseline-only).
    if !never && !finished && thin {
        return crate::specfence::LiveAct::Skip;
    }
    specfence
        .access_arms
        .decide(tx_idx, location_hash, writer, never, finished)
}

impl<S: Storage> Database for VmDb<'_, S> {
    type Error = ReadError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let location_hash = self.hash_basic(&address);
        let access_k = if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.specfence.access_log.note(self.tx_idx, location_hash)
        } else {
            0
        };
        self.consult_ungated_wait_once(address, location_hash, access_k)?;
        self.maybe_wait(address, location_hash, false)?;
        let resolve = self.resolve_read_overlay();

        // We return a mock for non-contract addresses (for lazy updates) to avoid
        // unnecessarily evaluating its balance here.
        if self.is_lazy {
            if location_hash == self.from_hash {
                return Ok(Some(AccountInfo {
                    nonce: self.tx.nonce,
                    balance: U256::MAX,
                    code: None,
                    code_hash: KECCAK_EMPTY,
                    account_id: None,
                }));
            } else if Some(location_hash) == self.to_hash {
                return Ok(None);
            }
        }

        // PCC / PrefixSkip FF. First-incarnation OptimisticRead uses the OCC MV walk.
        if resolve
            && !self.is_lazy
            && let Some((account, code_hash, origin)) = self.try_ff_basic(location_hash)
        {
            let read_origins = self.read_set.entry(location_hash).or_default();
            Self::push_origin(read_origins, origin.clone())?;
            self.deep_trace_read(
                location_hash,
                crate::specfence::LocationKind::Basic,
                Some(&origin),
            );
            self.specfence.metrics.record_journal_ff_hit();
            self.read_accounts
                .insert(location_hash, (account.clone(), code_hash));
            // Re-snap FF basics into rem. Without this, a second fail after
            // Rewind/ff_head leaves value_snap empty → full_from_0.
            let snap_origin = match &origin {
                ReadOrigin::MvMemory(v) => Some((v.tx_idx, v.tx_incarnation)),
                ReadOrigin::Storage => None,
            };
            self.maybe_note_value(
                location_hash,
                FfValue::Basic {
                    address,
                    basic: account.clone(),
                    code_hash,
                    origin: snap_origin,
                },
            );
            let code = if let Some(code_hash) = &code_hash {
                if let Some(code) = self.mv_memory.new_bytecodes.get(code_hash) {
                    Some(code.clone())
                } else {
                    match self.storage.code_by_hash(code_hash) {
                        Ok(code) => code.map(Bytecode::from),
                        Err(err) => return Err(ReadError::StorageError(err.to_string())),
                    }
                }
            } else {
                None
            };
            return Ok(Some(AccountInfo {
                balance: account.balance,
                nonce: account.nonce,
                code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
                code,
                account_id: None,
            }));
        }

        let read_origins = self.read_set.entry(location_hash).or_default();
        let has_prev_origins = !read_origins.is_empty();
        // We accumulate new origins to either:
        // - match with the previous origins to check consistency
        // - register origins on the first read
        let mut new_origins = SmallVec::new();

        let mut final_account = None;
        let mut balance_addition = U256::ZERO;
        // The sign of [balance_addition] since it can be negative for lazy senders.
        let mut positive_addition = true;
        let mut nonce_addition = 0;
        // WaitReleased may skip an Estimate tip to find older Data. If none
        // exists, falling through to pre-state + LackOfFund/NonceTooHigh
        // Blocking(tx-1) livelocks when tx-1 is already Validated (19469101
        // 1-core: hang-trace silent, 100% CPU inside try_execute_sf).
        let mut skipped_live_estimate: Option<TxIdx> = None;

        // Snapshot then drop the DashMap guard before any other `data.get`
        // (same-shard re-entry corrupts the heap — 19807137).
        let history: Vec<(TxIdx, MemoryEntry)> = if self.tx_idx > 0 {
            let _nest = crate::mv_memory::DataNest::enter("basic.history");
            self.mv_memory
                .data
                .get(&location_hash)
                .map(|written| {
                    written
                        .range(..self.tx_idx)
                        .map(|(k, v)| (*k, v.clone()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if !history.is_empty() {
            let mut iter = history.iter().rev();

            // Fully evaluate lazy updates
            loop {
                match iter.next() {
                    Some((blocking_idx, MemoryEntry::Estimate)) => {
                        // PCC / PrefixSkip OrderedDirtyRead. First-incarnation
                        // OptimisticRead/OCC Block on ESTIMATE (Block-STM).
                        if resolve
                            && new_origins.is_empty()
                            && balance_addition == U256::ZERO
                            && nonce_addition == 0
                        {
                            self.specfence.metrics.record_optimistic_read();
                            if self.specfence.mode == crate::ConcurrencyMode::SpecFence
                                && self.vis.needs_fence()
                            {
                                // Equivalent to SfMvMemory::read(WaitReleased|OrderedTip):
                                // skip the Estimate tip. Do not call SfMvMemory::read
                                // here — DashMap is not reentrant under this `get`.
                                self.specfence.metrics.record_sf_mv_read(self.vis);
                            }
                            if !self.specfence.scheduler.is_done(*blocking_idx) {
                                skipped_live_estimate =
                                    skipped_live_estimate.or(Some(*blocking_idx));
                            }
                            continue;
                        }
                        match live_writer_act(
                            &self.specfence,
                            self.tx_idx,
                            self.is_lazy,
                            address,
                            location_hash,
                            *blocking_idx,
                        ) {
                            crate::specfence::LiveAct::Skip => continue,
                            crate::specfence::LiveAct::Retry => {
                                return Err(ReadError::InconsistentRead);
                            }
                            crate::specfence::LiveAct::Block => {
                                if resolve {
                                    self.promote_on_conflict(address, location_hash);
                                    self.note_sf_mv_read();
                                } else {
                                    self.note_unpublished_raw(location_hash, *blocking_idx);
                                }
                                return Err(
                                    self.park_live_writer(location_hash, *blocking_idx)
                                );
                            }
                        }
                    }
                    Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) => {
                        if self
                            .mv_memory
                            .is_aborted_incarnation(*closest_idx, *tx_incarnation)
                        {
                            if resolve
                                && new_origins.is_empty()
                                && balance_addition == U256::ZERO
                                && nonce_addition == 0
                            {
                                self.specfence.metrics.record_optimistic_read();
                                if !self.specfence.scheduler.is_done(*closest_idx) {
                                    skipped_live_estimate =
                                        skipped_live_estimate.or(Some(*closest_idx));
                                }
                                continue;
                            }
                            match live_writer_act(
                                &self.specfence,
                                self.tx_idx,
                                self.is_lazy,
                                address,
                                location_hash,
                                *closest_idx,
                            ) {
                                crate::specfence::LiveAct::Skip => continue,
                                crate::specfence::LiveAct::Retry => {
                                    return Err(ReadError::InconsistentRead);
                                }
                                crate::specfence::LiveAct::Block => {
                                    if resolve {
                                        self.promote_on_conflict(address, location_hash);
                                    }
                                    return Err(
                                        self.park_live_writer(location_hash, *closest_idx)
                                    );
                                }
                            }
                        }
                        self.specfence.metrics.record_db_heavy_op();
                        // About to push a new origin
                        // Inconsistent: new origin will be longer than the previous!
                        if has_prev_origins && read_origins.len() == new_origins.len() {
                            return Err(ReadError::InconsistentRead);
                        }
                        let origin = ReadOrigin::MvMemory(TxVersion {
                            tx_idx: *closest_idx,
                            tx_incarnation: *tx_incarnation,
                        });
                        // Inconsistent: new origin is different from the previous!
                        if has_prev_origins
                            && unsafe { read_origins.get_unchecked(new_origins.len()) } != &origin
                        {
                            return Err(ReadError::InconsistentRead);
                        }
                        new_origins.push(origin);
                        match value {
                            MemoryValue::Basic(basic) => {
                                // TODO: Return [SelfDestructedAccount] if [basic] is
                                // [SelfDestructed]?
                                // For now we are betting on [code_hash] triggering the
                                // sequential fallback when we read a self-destructed contract.
                                final_account = Some(basic.clone());
                                break;
                            }
                            MemoryValue::LazyRecipient(addition) => {
                                if positive_addition {
                                    balance_addition = balance_addition.saturating_add(*addition);
                                } else {
                                    positive_addition = *addition >= balance_addition;
                                    balance_addition = balance_addition.abs_diff(*addition);
                                }
                            }
                            MemoryValue::LazySender(subtraction) => {
                                if positive_addition {
                                    positive_addition = balance_addition >= *subtraction;
                                    balance_addition = balance_addition.abs_diff(*subtraction);
                                } else {
                                    balance_addition =
                                        balance_addition.saturating_add(*subtraction);
                                }
                                nonce_addition += 1;
                            }
                            _ => return Err(ReadError::InvalidMemoryValueType),
                        }
                    }
                    None => {
                        break;
                    }
                }
            }
        }

        // Fall back to storage
        if final_account.is_none() {
            if let Some(w) = skipped_live_estimate {
                match live_writer_act(
                    &self.specfence,
                    self.tx_idx,
                    self.is_lazy,
                    address,
                    location_hash,
                    w,
                ) {
                    crate::specfence::LiveAct::Skip => {}
                    crate::specfence::LiveAct::Retry => {
                        return Err(ReadError::InconsistentRead);
                    }
                    crate::specfence::LiveAct::Block => {
                        if resolve {
                            self.promote_on_conflict(address, location_hash);
                            self.note_sf_mv_read();
                        } else {
                            self.note_unpublished_raw(location_hash, w);
                        }
                        return Err(self.park_live_writer(location_hash, w));
                    }
                }
            }
            self.specfence.metrics.record_db_heavy_op();
            // Populate [Storage] on the first read
            if !has_prev_origins {
                new_origins.push(ReadOrigin::Storage);
            }
            // Inconsistent: previous origin is longer or didn't read
            // from storage for the last origin.
            else if read_origins.len() != new_origins.len() + 1
                || read_origins.last() != Some(&ReadOrigin::Storage)
            {
                return Err(ReadError::InconsistentRead);
            }
            final_account = match self.storage.basic(&address) {
                Ok(Some(basic)) => Some(basic),
                Ok(None) => (balance_addition > U256::ZERO).then(AccountBasic::default),
                Err(err) => return Err(ReadError::StorageError(err.to_string())),
            };
        }

        // Populate read origins on the first read.
        // Otherwise [read_origins] matches [new_origins] already.
        if !has_prev_origins {
            *read_origins = new_origins;
        }

        // Deep: one DB-effect for this basic() call; emit edge per MV origin observed.
        if let Some(fg) = self.specfence.finegrain {
            if fg.deep_enabled() {
                let origins_now = self
                    .read_set
                    .get(&location_hash)
                    .cloned()
                    .unwrap_or_default();
                let mv_origins: Vec<_> = origins_now
                    .iter()
                    .filter_map(|o| match o {
                        ReadOrigin::MvMemory(v) => Some((v.tx_idx, v.tx_incarnation)),
                        _ => None,
                    })
                    .collect();
                if mv_origins.is_empty() {
                    fg.deep_note_db_read(
                        self.tx_idx,
                        self.tx_incarnation,
                        None,
                        location_hash,
                        crate::specfence::LocationKind::Basic,
                        true,
                    );
                } else {
                    for (i, (ptx, pinc)) in mv_origins.into_iter().enumerate() {
                        let kind = if i == 0 {
                            crate::specfence::LocationKind::Basic
                        } else {
                            crate::specfence::LocationKind::BasicLazy
                        };
                        fg.deep_note_db_read(
                            self.tx_idx,
                            self.tx_incarnation,
                            Some((ptx, pinc)),
                            location_hash,
                            kind,
                            i == 0,
                        );
                    }
                }
            }
        }

        if let Some(mut account) = final_account {
            // Check sender nonce
            account.nonce += nonce_addition;
            if self.has_nonce && location_hash == self.from_hash && self.tx.nonce != account.nonce {
                let pred_done =
                    self.tx_idx > 0 && self.specfence.scheduler.is_done(self.tx_idx - 1);
                let leftover_min = self.specfence.ready_edges.is_live_leftover_min(self.tx_idx);
                let pred_passed = self.tx_idx > 0
                    && self
                        .specfence
                        .ready_edges
                        .leftover_min_skips_blocker(self.tx_idx - 1);
                if leftover_min && (pred_done || pred_passed) {
                    // leftover_min must commit on a done prefix. Blocking(tx-1)
                    // parks fail, leftover_min stays Executing, heal mills
                    // (19807137 leftover_min=514 n_unf=198).
                } else if self.tx_idx > 0 {
                    // Non-leftover still Blocks on tx-1 even when pred is
                    // done (OCC). `!pred_done` here made tx 120 InvalidNonce
                    // abort 19807137 first-SF.
                    self.promote_on_conflict(address, location_hash);
                    return Err(ReadError::Blocking(self.tx_idx - 1));
                } else {
                    return Err(ReadError::InvalidNonce(self.tx_idx));
                }
            }

            // Fully evaluate the account and register it to read cache
            // to later check if they have changed (been written to).
            if positive_addition {
                account.balance = account.balance.saturating_add(balance_addition);
            } else {
                account.balance = account.balance.saturating_sub(balance_addition);
            };

            let code_hash = if Some(location_hash) == self.to_hash {
                self.to_code_hash
            } else {
                self.get_code_hash(address)?
            };
            let code = if let Some(code_hash) = &code_hash {
                if let Some(code) = self.mv_memory.new_bytecodes.get(code_hash) {
                    Some(code.clone())
                } else {
                    match self.storage.code_by_hash(code_hash) {
                        Ok(code) => code.map(Bytecode::from),
                        Err(err) => return Err(ReadError::StorageError(err.to_string())),
                    }
                }
            } else {
                None
            };
            self.read_accounts
                .insert(location_hash, (account.clone(), code_hash));

            // M1b: cache basics for FF (single-origin origin) and Iter6 value-stable
            // RebindOnly (multi-origin lazy: snap balance+nonce with origin=None so
            // try_ff_basic refuses — avoids seq≠par on lazy chains).
            let origins = self.read_set.get(&location_hash);
            let single = origins.is_some_and(|o| o.len() == 1);
            let origin = if single {
                match origins.and_then(|o| o.first()) {
                    Some(ReadOrigin::MvMemory(v)) => Some((v.tx_idx, v.tx_incarnation)),
                    Some(ReadOrigin::Storage) | None => None,
                }
            } else {
                None
            };
            // Snap even on OptimisticRead PE-on — WaitForDependency resume needs prefix values
            // (`resolve` overlay was leaving value_snap empty → FullAbortReexecute theater).
            self.maybe_note_value(
                location_hash,
                FfValue::Basic {
                    address,
                    basic: account.clone(),
                    code_hash,
                    origin,
                },
            );
            if resolve {
                self.maybe_early_val(address, location_hash)?;
            }
            return Ok(Some(AccountInfo {
                balance: account.balance,
                nonce: account.nonce,
                code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
                code,
                account_id: None,
            }));
        }

        if resolve {
            self.maybe_early_val(address, location_hash)?;
        }
        Ok(None)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        match self
            .storage
            .code_by_hash(&code_hash)
            .map_err(|err| ReadError::StorageError(err.to_string()))?
        {
            Some(evm_code) => Ok(Bytecode::from(evm_code)),
            None => Ok(Bytecode::default()),
        }
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let location_hash = hash_deterministic(MemoryLocation::Storage(address, index));
        let access_k = if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.specfence.access_log.note(self.tx_idx, location_hash)
        } else {
            0
        };
        self.consult_ungated_wait_once(address, location_hash, access_k)?;
        self.maybe_wait(address, location_hash, true)?;
        let resolve = self.resolve_read_overlay();

        // PCC-only FF. OptimisticRead/OCC never consult rem journals.
        if resolve && let Some((value, origin)) = self.try_ff_storage(location_hash) {
            let read_origins = self.read_set.entry(location_hash).or_default();
            Self::push_origin(read_origins, origin.clone())?;
            self.deep_trace_read(
                location_hash,
                crate::specfence::LocationKind::Storage,
                Some(&origin),
            );
            self.specfence.metrics.record_journal_ff_hit();
            if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                self.maybe_note_value(
                    location_hash,
                    FfValue::Storage {
                        address,
                        slot: index,
                        value,
                        origin: match self.read_set.get(&location_hash).and_then(|o| o.last()) {
                            Some(ReadOrigin::MvMemory(v)) => Some((v.tx_idx, v.tx_incarnation)),
                            _ => None,
                        },
                    },
                );
            }
            return Ok(value);
        }

        let read_origins = self.read_set.entry(location_hash).or_default();

        // Snapshot the closest prior entry under one `data.get`, then drop
        // the DashMap guard before `last_data_before` / `maybe_early_val`
        // (same-map re-entry is the 19807137 `double free or corruption`).
        enum StorageTip {
            Live {
                idx: TxIdx,
                inc: crate::TxIncarnation,
                value: U256,
            },
            SkipTo {
                closest_idx: TxIdx,
                prior: Option<(TxIdx, crate::TxIncarnation, U256)>,
                estimate: bool,
            },
            BadType,
        }
        let tip = if self.tx_idx > 0 {
            let _nest = crate::mv_memory::DataNest::enter("storage.tip");
            self.mv_memory.data.get(&location_hash).and_then(|written| {
                let (idx, entry) = written.range(..self.tx_idx).next_back()?;
                match entry {
                    MemoryEntry::Data(inc, MemoryValue::Storage(v)) => {
                        if self.mv_memory.is_aborted_incarnation(*idx, *inc) {
                            let prior = crate::mv_memory::MvMemory::last_live_storage_in(
                                &written,
                                self.tx_idx,
                                |i, inc| self.mv_memory.is_aborted_incarnation(i, inc),
                            );
                            Some(StorageTip::SkipTo {
                                closest_idx: *idx,
                                prior,
                                estimate: false,
                            })
                        } else {
                            Some(StorageTip::Live {
                                idx: *idx,
                                inc: *inc,
                                value: *v,
                            })
                        }
                    }
                    MemoryEntry::Estimate => {
                        let prior = crate::mv_memory::MvMemory::last_live_storage_in(
                            &written,
                            self.tx_idx,
                            |i, inc| self.mv_memory.is_aborted_incarnation(i, inc),
                        );
                        Some(StorageTip::SkipTo {
                            closest_idx: *idx,
                            prior,
                            estimate: true,
                        })
                    }
                    _ => Some(StorageTip::BadType),
                }
            })
        } else {
            None
        };

        match tip {
            Some(StorageTip::Live { idx, inc, value }) => {
                self.specfence.metrics.record_db_heavy_op();
                let origin = ReadOrigin::MvMemory(TxVersion {
                    tx_idx: idx,
                    tx_incarnation: inc,
                });
                Self::push_origin(read_origins, origin.clone())?;
                self.deep_trace_read(
                    location_hash,
                    crate::specfence::LocationKind::Storage,
                    Some(&origin),
                );
                self.maybe_note_value(
                    location_hash,
                    FfValue::Storage {
                        address,
                        slot: index,
                        value,
                        origin: Some((idx, inc)),
                    },
                );
                if resolve {
                    self.maybe_early_val(address, location_hash)?;
                }
                return Ok(value);
            }
            Some(StorageTip::SkipTo {
                closest_idx,
                prior,
                estimate,
            }) => {
                if resolve {
                    if let Some((idx, inc, v2)) = prior {
                        self.specfence.metrics.record_optimistic_read();
                        self.specfence.metrics.record_db_heavy_op();
                        let origin = ReadOrigin::MvMemory(TxVersion {
                            tx_idx: idx,
                            tx_incarnation: inc,
                        });
                        Self::push_origin(read_origins, origin.clone())?;
                        self.deep_trace_read(
                            location_hash,
                            crate::specfence::LocationKind::Storage,
                            Some(&origin),
                        );
                        self.maybe_note_value(
                            location_hash,
                            FfValue::Storage {
                                address,
                                slot: index,
                                value: v2,
                                origin: Some((idx, inc)),
                            },
                        );
                        self.maybe_early_val(address, location_hash)?;
                        return Ok(v2);
                    }
                    if estimate {
                        // No prior Data. A live Estimate writer is unpublished —
                        // do not read pre-state (LackOfFund/NonceTooHigh then
                        // Blocking(tx-1) livelocks when tx-1 is already done).
                        // NeverWait / a second WaitOnce may fall through; leftover
                        // still parks (`live_writer_act`).
                        self.specfence.metrics.record_optimistic_read();
                        if !self.specfence.scheduler.is_done(closest_idx) {
                            match live_writer_act(
                                &self.specfence,
                                self.tx_idx,
                                self.is_lazy,
                                address,
                                location_hash,
                                closest_idx,
                            ) {
                                crate::specfence::LiveAct::Skip => {}
                                crate::specfence::LiveAct::Retry => {
                                    return Err(ReadError::InconsistentRead);
                                }
                                crate::specfence::LiveAct::Block => {
                                    self.promote_on_conflict(address, location_hash);
                                    return Err(
                                        self.park_live_writer(location_hash, closest_idx)
                                    );
                                }
                            }
                        }
                    } else {
                        match live_writer_act(
                            &self.specfence,
                            self.tx_idx,
                            self.is_lazy,
                            address,
                            location_hash,
                            closest_idx,
                        ) {
                            crate::specfence::LiveAct::Skip => {}
                            crate::specfence::LiveAct::Retry => {
                                return Err(ReadError::InconsistentRead);
                            }
                            crate::specfence::LiveAct::Block => {
                                self.promote_on_conflict(address, location_hash);
                                return Err(self.park_live_writer(location_hash, closest_idx));
                            }
                        }
                    }
                } else if estimate {
                    match live_writer_act(
                        &self.specfence,
                        self.tx_idx,
                        self.is_lazy,
                        address,
                        location_hash,
                        closest_idx,
                    ) {
                        crate::specfence::LiveAct::Skip => {}
                        crate::specfence::LiveAct::Retry => {
                            return Err(ReadError::InconsistentRead);
                        }
                        crate::specfence::LiveAct::Block => {
                            self.promote_on_conflict(address, location_hash);
                            self.note_unpublished_raw(location_hash, closest_idx);
                            return Err(self.park_live_writer(location_hash, closest_idx));
                        }
                    }
                } else {
                    match live_writer_act(
                        &self.specfence,
                        self.tx_idx,
                        self.is_lazy,
                        address,
                        location_hash,
                        closest_idx,
                    ) {
                        crate::specfence::LiveAct::Skip => {}
                        crate::specfence::LiveAct::Retry => {
                            return Err(ReadError::InconsistentRead);
                        }
                        crate::specfence::LiveAct::Block => {
                            return Err(self.park_live_writer(location_hash, closest_idx));
                        }
                    }
                }
            }
            Some(StorageTip::BadType) => return Err(ReadError::InvalidMemoryValueType),
            None => {}
        }

        // Fall back to storage
        self.specfence.metrics.record_db_heavy_op();
        Self::push_origin(read_origins, ReadOrigin::Storage)?;
        self.deep_trace_read(
            location_hash,
            crate::specfence::LocationKind::Storage,
            Some(&ReadOrigin::Storage),
        );
        let value = self
            .storage
            .storage(&address, &index)
            .map_err(|err| ReadError::StorageError(err.to_string()))?;
        self.maybe_note_value(
            location_hash,
            FfValue::Storage {
                address,
                slot: index,
                value,
                origin: None,
            },
        );
        if resolve {
            self.maybe_early_val(address, location_hash)?;
        }
        Ok(value)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.storage
            .block_hash(&number)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

// Per-worker execution VM. Holds all block-level state and a reusable EVM.
pub(crate) struct Vm<'a, S: Storage, C: PevmChain> {
    // Shared block-level state
    chain: &'a C,
    is_eip_161_enabled: bool,
    block_env: &'a BlockEnv,
    txs: &'a [C::EvmTx],
    mv_memory: &'a MvMemory,
    specfence: SpecFenceCtx<'a>,
    beneficiary_location_hash: MemoryLocationHash,
    // Dedicated EVM for the worker, reset before each transaction exectution.
    evm: C::Evm<VmDb<'a, S>>,
}

impl<'a, S: Storage, C: PevmChain> Vm<'a, S, C> {
    /// A2 progressive DAG: published Data notifies Blocking waiters (not SoftWait Soft).
    /// Ready transition stays in `finish_execution` (dependents drain) — do not
    /// `try_ready` here or release builds double-incarnate and corrupt.
    /// SfMvMemory: mark Released tips and wake exact WaitOnce waiters.
    /// ChainSpine: also wake planted nearest succ into wave → `Q_released`
    /// (schedule Avoid on Released — no Blocking park).
    fn wake_on_data_publish(
        &self,
        writer: crate::TxIdx,
        incarnation: crate::TxIncarnation,
        locs: &[crate::MemoryLocationHash],
    ) {
        for &loc in locs {
            let thin = self.specfence.scheduler.block_size() <= crate::specfence::THIN_SHELL_N;
            // Thin WaitOnce/crit DashMap tip; large ChainSpineTip only.
            if thin
                && (self.specfence.access_arms.is_crit_loc(loc)
                    || self.specfence.access_arms.is_wait_once(loc))
            {
                let exact = self
                    .specfence
                    .sf_tips
                    .publish_data(loc, writer, incarnation);
                for c in exact {
                    self.specfence.wave.push_ready(c);
                }
            } else if self.specfence.sf_tips.is_chain_loc(loc) {
                let exact = self
                    .specfence
                    .sf_tips
                    .publish_data(loc, writer, incarnation);
                for c in exact {
                    self.specfence.wave.push_ready(c);
                }
                self.specfence
                    .ready_edges
                    .wake_planted_on_publish(writer, self.specfence.wave);
            }
            self.specfence.sketch.push_spine(loc, writer);
            self.specfence.ready_edges.note_published(loc, writer);
            let k = self.specfence.learner.dominant_k(loc);
            self.specfence.lanes.release(loc, k.max(1), writer);
            let _ = self.specfence.wave.wake_location(loc);
            let _ = self.specfence.dag.wake_on_data(loc, writer);
            self.specfence.metrics.record_data_publish_wake();
        }
    }

    /// True when Soft=0 ChainSpine should schedule-defer instead of Aborting.
    #[inline]
    pub(crate) fn chain_spine_schedule_defer(&self, location: MemoryLocationHash) -> bool {
        self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && self.specfence.sf_tips.is_chain_loc(location)
    }

    pub(crate) fn new(
        chain: &'a C,
        spec_id: C::EvmSpecId,
        block_env: &'a BlockEnv,
        txs: &'a [C::EvmTx],
        storage: &'a S,
        mv_memory: &'a MvMemory,
        specfence: SpecFenceCtx<'a>,
    ) -> Self {
        // The DB is initialised with mock values; each transaction execution
        // [VmDb::set_tx] the intended transaction before executing.
        let db = VmDb {
            storage,
            mv_memory,
            specfence,
            tx_idx: 0,
            tx_incarnation: 0,
            // SAFETY: txs is non-empty (checked by the caller before spawning threads).
            tx: chain.tx_env(unsafe { txs.get_unchecked(0) }),
            from_hash: 0,
            to_hash: None,
            to_code_hash: None,
            is_lazy: false,
            optimistic_majority_lazy: false,
            optimistic_skip_gate: false,
            sf_occ_shaped: false,
            vis: VisibilityPolicy::Opt,
            pcc_armed: Cell::new(false),
            optimistic_read_this_tx: Cell::new(0),
            pcc_this_tx: Cell::new(0),
            has_nonce: true,
            // Unless it is a raw transfer that is lazy updated, we'll
            // read at least from the sender and recipient accounts.
            read_set: ReadSet::with_capacity_and_hasher(2, BuildIdentityHasher::default()),
            read_accounts: HashMap::with_capacity_and_hasher(2, BuildIdentityHasher::default()),
        };
        Self {
            chain,
            is_eip_161_enabled: chain.is_eip_161_enabled(spec_id),
            block_env,
            txs,
            mv_memory,
            specfence,
            beneficiary_location_hash: hash_deterministic(MemoryLocation::Basic(
                block_env.beneficiary,
            )),
            evm: chain.build_evm(spec_id, block_env.clone(), db),
        }
    }

    /// Hinted Wait admission: previous `from`/`to` writer that is not done yet.
    pub(crate) fn hinted_wait_blocker(&self, tx_idx: TxIdx) -> Option<(TxIdx, Address)> {
        if !crate::specfence::hinted_wait_enabled(self.specfence.mode) {
            return None;
        }
        // V5-P0: SpecFence should_wait_account is always false (account Wait stub).
        let tx = self.chain.tx_env(unsafe { self.txs.get_unchecked(tx_idx) });
        if let Some(prev) = self
            .specfence
            .wait_blocker(&self.mv_memory.regions, &tx.caller, tx_idx)
        {
            return Some((prev, tx.caller));
        }
        if let Some(to) = tx.kind.to()
            && let Some(prev) = self
                .specfence
                .wait_blocker(&self.mv_memory.regions, to, tx_idx)
        {
            return Some((prev, *to));
        }
        None
    }

    /// SpecFence M2/P4: location + SoftWait `k` of WaitHard that returned Blocking.
    pub(crate) fn take_pending_park(&self) -> Option<crate::specfence::PendingPark> {
        self.specfence.wave.take_pending_park()
    }

    /// SpecFence M2: location of WaitHard that returned Blocking (if any).
    pub(crate) fn take_pending_park_location(&self) -> Option<crate::MemoryLocationHash> {
        self.take_pending_park().map(|p| p.location)
    }

    /// P4: apply SoftWait park resume intent before re-execute (journal still parked).
    ///
    /// Arms RewindTo/FF when a checkpoint exists before SoftWait `k`; else FullAbortReexecute.
    pub(crate) fn try_apply_park_resume(
        &self,
        tx_idx: crate::TxIdx,
        wave: &crate::specfence::WaveParkTable,
    ) {
        let Some(intent) = wave.take_resume_intent(tx_idx) else {
            return;
        };
        // Dig: SoftWait (FenceGraph) wakes — not EarlyAbort-only parks.
        if self.specfence.partial_retry.take_softwait_parked(tx_idx) {
            self.specfence.partial_retry.mark_post_softwait_wake(tx_idx);
        }
        // Three-pillar Await@a wake credit (BO-until-Validated on hot ℓ).
        if self.specfence.partial_retry.take_await_at_a_parked(tx_idx) {
            self.specfence
                .partial_retry
                .mark_post_await_at_a_wake(tx_idx);
        }
        let kind = self
            .specfence
            .partial_retry
            .try_arm_wait_for_dependency_resume_at_k(tx_idx, intent.armed_at_k);
        match kind {
            crate::specfence::ParkResumeKind::ResumeAtK { .. } => {
                wave.note_park_resume_at_k();
                self.specfence.metrics.record_rewind_to_cp();
            }
            crate::specfence::ParkResumeKind::FullAbortReexecute => {
                wave.note_park_resume_full_abort_reexecute();
            }
        }
    }

    pub(crate) fn record_wait_admission(&self, address: Address) {
        self.specfence.metrics.record_wait(address);
    }

    /// G4/AEC: credit LiveLearner wait_useful + arm→wake latency for SoftWait wakes.
    pub(crate) fn ready_edges(&self) -> &crate::specfence::ReadyEdgeTable {
        self.specfence.ready_edges
    }

    pub(crate) fn record_wait_for_dependency(&self) {
        self.specfence.metrics.record_wait_for_dependency();
    }

    pub(crate) fn record_wait_for_full_abort(&self) {
        self.specfence.metrics.record_wait_for_full_abort();
    }

    /// L6: one hot-path A0/A1 execute tick (not access-level museum counters).
    pub(crate) fn note_execute_edge(&self, tx_idx: crate::TxIdx) {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence {
            return;
        }
        if self.specfence.ready_edges.was_queued(tx_idx) {
            self.specfence.metrics.record_edge_ordered_admit();
        } else {
            self.specfence.metrics.record_edge_optimistic_read();
        }
    }

    pub(crate) fn note_hot_reexec_ns(&self, tx_idx: crate::TxIdx, ns: u64) {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence || ns == 0 {
            return;
        }
        self.specfence.metrics.record_reexec_ns(ns);
        if let Some(p) = self.specfence.policy {
            let loc = p.conflict_of(tx_idx).map(|n| n.location);
            p.note_reexec_ns_at(loc, ns);
        }
    }

    pub(crate) fn release_ready_edges(
        &self,
        writer: TxIdx,
        wave: &crate::specfence::WaveParkTable,
    ) {
        self.specfence.ready_edges.note_producer_done(writer, wave);
        self.specfence.producer_stages.note_done(writer);
    }

    pub(crate) fn credit_softwait_wakes(&self, fence: &crate::specfence::FenceGraph) {
        for loc in fence.drain_wake_useful_locs() {
            self.specfence.learner.note_wait_useful(loc);
        }
        for (loc, ns) in fence.drain_wake_latencies() {
            self.specfence.learner.note_wait_latency(loc, ns);
        }
    }

    fn promote_region(&self, location: MemoryLocationHash, address: Option<Address>) {
        // WW contention → intra-block Wait; mild once-per-block Bayes conflict.
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            if self.specfence.bayes.observe_conflict_location(location) {
                self.specfence.metrics.record_bayes_conflict();
                if let Some(address) = address {
                    if address != self.specfence.beneficiary {
                        self.specfence.bayes.observe_conflict_account_n(address, 3);
                    }
                }
            }
            self.specfence
                .promote_from_bayes(&self.mv_memory.regions, location, address);
            return;
        }
        if self.mv_memory.regions.promote_location(location) {
            self.specfence.metrics.record_promotion(address);
        }
        if let Some(address) = address {
            if address == self.specfence.beneficiary {
                return;
            }
            // G5: PCC/legacy only (SpecFence returned above).
            self.mv_memory.regions.promote_account(address);
            self.specfence.metrics.mark_hot(address);
        }
    }

    fn promote_if_multi_writer(&self, address: Address, location: Option<MemoryLocationHash>) {
        // SpecFence v5: never promote Wait from from/to writer_count hints.
        // SoftWait arms only via choose_action; seed_wait_regions is PCC-only.
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            return;
        }
        if address == self.specfence.beneficiary {
            return;
        }
        if self.specfence.hints.writer_count(&address) < 2 {
            return;
        }
        if let Some(location) = location {
            self.promote_region(location, Some(address));
        } else {
            // G5: PCC/legacy only (SpecFence returned above).
            self.mv_memory.regions.promote_account(address);
            self.specfence.metrics.mark_hot(address);
        }
    }

    // Execute a transaction. This can read from memory but cannot modify any state.
    // A successful execution returns:
    //   - A write-set consisting of memory locations and their updated values.
    //   - A read-set consisting of memory locations and their origins.
    //
    // An execution may observe a read dependency on a lower transaction. This happens
    // when the last incarnation of the dependency wrote to a memory location that
    // this transaction reads, but it aborted before the read. In this case, the
    // dependency index is returned via [blocking_tx_idx]. An execution task for this
    // transaction is re-scheduled after the blocking dependency finishes its
    // next incarnation.
    //
    // When a transaction attempts to write a value to a location, the location and
    // value are added to the write set, possibly replacing a pair with a prior value
    // (if it is not the first time the transaction wrote to this location during the
    // execution).
    pub(crate) fn execute(
        &mut self,
        tx_version: &TxVersion,
        result_slot: &mut Option<PevmTxExecutionResult>,
    ) -> Result<FinishExecFlags, VmExecutionError> {
        // SAFETY: A correct scheduler would guarantee this index to be inbound.
        let full_tx = unsafe { self.txs.get_unchecked(tx_version.tx_idx) };
        let tx = self.chain.tx_env(full_tx);

        let from_hash = hash_deterministic(MemoryLocation::Basic(tx.caller));
        let to_hash = tx
            .kind
            .to()
            .map(|to| hash_deterministic(MemoryLocation::Basic(*to)));

        let has_nonce = self.chain.has_nonce(&mut self.evm, full_tx);

        // Prepare state for execution
        {
            let ctx = self.evm.ctx();

            ctx.db_mut()
                .set_tx(
                    tx_version.tx_idx,
                    tx,
                    from_hash,
                    to_hash,
                    has_nonce,
                    tx_version.tx_incarnation,
                )
                .map_err(VmExecutionError::from)?;

            ctx.set_tx(full_tx.clone());

            // We reset the journal when we finalise it into the result state on a
            // successful execution but not on errors. Always reset here to be sure.
            ctx.journal_mut().clear();
        }

        let sf_occ_shaped = self.evm.ctx().db().sf_occ_shaped;
        // Museum / tip only when this tx is a WaitOnce consumer or producer.
        if !sf_occ_shaped {
            self.note_execute_edge(tx_version.tx_idx);
        }

        // SpecFence write path: early version tip + live_writer.
        // Thin: DashMap WaitOnce/crit (skip when OCC-shaped non-producer).
        // Large: ChainSpineTip only (sticky≥32).
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            let thin = self.specfence.scheduler.block_size() <= crate::specfence::THIN_SHELL_N;
            let crit = self.specfence.access_arms.crit_loc_hash();
            let install_tip = thin
                && (!sf_occ_shaped
                    || self
                        .specfence
                        .access_arms
                        .is_wait_once_producer(tx_version.tx_idx));
            if install_tip {
                let prior = self.mv_memory.write_locations(tx_version.tx_idx);
                for &loc in &prior {
                    if self.specfence.access_arms.is_crit_loc(loc)
                        || self.specfence.access_arms.is_wait_once(loc)
                    {
                        self.specfence.sf_tips.install_version_tip(
                            loc,
                            tx_version.tx_idx,
                            tx_version.tx_incarnation,
                        );
                    }
                }
                if crit != u64::MAX
                    && self.specfence.access_arms.is_wait_once(crit)
                    && !prior.iter().any(|&l| l == crit)
                {
                    self.specfence.sf_tips.install_version_tip(
                        crit,
                        tx_version.tx_idx,
                        tx_version.tx_incarnation,
                    );
                }
            } else if !thin && self.specfence.sf_tips.is_chain_loc(crit) {
                // ChainSpineTip claim — O(1) flag, not WaitOnce DashMap mill.
                self.specfence.sf_tips.chain_claim(
                    tx_version.tx_idx,
                    tx_version.tx_incarnation,
                );
            }
        }

        // SpecFence-native resume: RewindTo (SuffixRepair / SoftWait wake) takes the
        // resume path on Lean too — do NOT gate on `!lean`. Journal FF (`try_ff_*`)
        // already keys off table `is_rewind_resume`; this also seeds read origins,
        // prefers `record_resume`, and may narrow-arm hang-free absolute jump.
        let optimistic_ungated_exec = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && self
                .specfence
                .policy
                .is_some_and(|p| p.skip_ungated_tx_path_tax())
            && !self.specfence.ready_edges.is_gated(tx_version.tx_idx);
        // OCC-shaped thin: force lean without engagement atomics.
        let lean = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && (sf_occ_shaped || self.specfence.engagement.begin_tx(tx_version.tx_idx));
        let repair_armed = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && !optimistic_ungated_exec
            && (self
                .specfence
                .partial_retry
                .is_rewind_resume(tx_version.tx_idx)
                || self.specfence.partial_retry.has_ff_head(tx_version.tx_idx));
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            if repair_armed {
                self.specfence.metrics.record_pcc_kernel_exec();
            } else {
                self.specfence.metrics.record_occ_kernel_exec();
            }
        }
        let rewind_resume = repair_armed
            && self
                .specfence
                .partial_retry
                .is_rewind_resume(tx_version.tx_idx);
        if rewind_resume {
            // M1b/M1d: journal FF in set_tx; optional live PC arm inside inspect_run.
            self.specfence.metrics.record_resume();
        } else if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.specfence.metrics.record_evm_entry();
            if repair_armed {
                let _ = self
                    .specfence
                    .partial_retry
                    .push_checkpoint(tx_version.tx_idx, CheckpointKind::CallEntry);
            }
        }

        // Hang-free SuffixRepair prefix skip + live-snap capture (Iter2):
        // Lean EffectBoundary snaps are often lite → jump_is_safe never arms.
        // Open narrow inspect_run (no whole-block SPECFENCE_ENABLE_INSPECT) when:
        //   (a) RewindTo already jump_is_safe (absolute jump), or
        //   (b) one-shot Storage+write_replay live_prime (`needs_live_capture`)
        //       — NOT bare force_ordered_admit (that 4× evm_entries / wall↑ on 597).
        // Capture plants live jump_snap; pevm delays fb escalate once so the
        // next SuffixRepair can arm absolute jump. Honor SPECFENCE_ABSOLUTE_JUMP=0.
        let ff_cont = if rewind_resume {
            self.specfence
                .partial_retry
                .ff_continuation(tx_version.tx_idx)
        } else {
            None
        };
        let jump_disabled = rewind_resume
            && self
                .specfence
                .partial_retry
                .is_jump_disabled(tx_version.tx_idx);
        let storage_prefix = ff_cont.as_ref().is_some_and(|cont| {
            cont.values
                .values()
                .any(|v| matches!(v, FfValue::Storage { .. }))
        });
        let basic_prefix = ff_cont.as_ref().is_some_and(|cont| {
            cont.values
                .values()
                .any(|v| matches!(v, FfValue::Basic { .. }))
        });
        // Hang-free jump/prime when Storage or Basic FF values exist (M1f Basic-only).
        let ff_prefix = storage_prefix || basic_prefix;
        // Iter5: capture window plants SSTORE tips with WaitHard demoted (hang-free).
        // Absolute jump stays research-only — Lean jump broke seq≡par (empty memory).
        // Production resolve bet: head-FF retain across escalate (write-prefix DB skip).
        let needs_capture = rewind_resume
            && self
                .specfence
                .partial_retry
                .needs_live_capture(tx_version.tx_idx);
        let mut ff_cont = ff_cont;
        if rewind_resume && ff_prefix {
            // Iter28: Lean LAST_SNAP is worker-TLS; OrderedAdmit-snap TLS now clears it,
            // but attach here still races steal. ff_continuation already has
            // jump_snap from arm_rewind live_boundaries — skip Lean attach.
            if !lean {
                attach_current_live_snap(tx_version.tx_idx, self.specfence.partial_retry);
            }
            ff_cont = self
                .specfence
                .partial_retry
                .ff_continuation(tx_version.tx_idx);
        }
        // Iter7/8 memory-lite jump: non-empty memory (SSTORE plant path).
        let memory_lite_ok = ff_cont.as_ref().is_some_and(|cont| {
            cont.jump_snap
                .as_ref()
                .is_some_and(|s| s.is_live_capture() && !s.memory.is_empty())
        });
        // Iter19: read-only OrderedAdmit/EffectBoundary snap (sstore_index=0, !post_sstore,
        // no write_replays) — certified-prefix end before k_fail on RAW-read fails.
        // Empty memory OK when jump_is_safe read-prefix gates pass (memory may still
        // be present from thin OrderedAdmit-snap clone ≤8KiB).
        let read_prefix_ok = ff_cont.as_ref().is_some_and(|cont| {
            cont.jump_snap.as_ref().is_some_and(|s| {
                s.is_live_capture()
                    && s.sstore_index == 0
                    && !s.post_sstore
                    && cont.write_replays.is_empty()
                    && !cont.effects.iter().any(|e| e.mode == AccessMode::Write)
            })
        });
        let suffix_jump_eligible = lean
            && rewind_resume
            && suffix_repair_jump_env_ok()
            && !jump_disabled
            && ff_prefix
            && (memory_lite_ok || read_prefix_ok)
            && ff_cont.as_ref().is_some_and(|cont| {
                absolute_jump_eligible(tx_version.tx_idx, self.specfence.partial_retry, cont)
            });
        // Iter20: OrderedAdmit-snap tips at k<k_fail are consumable hang-free when JUMP is
        // opt-in. Iter19 gated `!memory_lite_ok` → aj=0 on mainnet (OrderedAdmit snaps clone
        // ≤8KiB memory) while empty-memory Lean edges hung under stale FF origin
        // seed. Fix: allow read-prefix jump *with* memory; Validated-safe origin
        // seed (refuse jump if any certified origin is Estimate/unstable); credit
        // fallback when jump not armed. Production JUMP/SNAP stay OFF (no default tax).
        // SoftWait Soft=0. Stock SSTORE. No mega-fan yield.
        // Iter24: JUMP follows OrderedAdmitSnapMode (ResumePath/Mass default-on) unless
        // SPECFENCE_BIND_SNAP_JUMP=0. Mass SNAP still opt-in (`SPECFENCE_BIND_SNAP=1`).
        // Refuse-if-stale + Validated-prefix gates remain. SoftWait Soft=0.
        let ordered_admit_snap_jump_env = ordered_admit_snap_jump_enabled();
        // Iter21: top-level/shallow OrderedAdmit tips only (call_depth≤1). Deeper ERC-20
        // SLOAD tips fail apply_to_interp depth match or restore wrong frame →
        // seeded origins without PC skip → seq≠par (aj metric was also blind).
        let ordered_admit_depth_ok = ff_cont
            .as_ref()
            .is_some_and(|cont| cont.jump_snap.as_ref().is_some_and(|s| s.call_depth <= 1));
        // Iter22: refuse OrderedAdmit jump when snap omitted memory bytes (≤8KiB cap) but
        // MemoryGas still reports words — restore would wipe live memory → seq≠par.
        let ordered_admit_memory_ok = ff_cont.as_ref().is_some_and(|cont| {
            cont.jump_snap
                .as_ref()
                .is_some_and(|s| s.memory_words == 0 || !s.memory.is_empty())
        });
        // Iter22: require a real-looking OrderedAdmit tip (pc/gas/stack live).
        let ordered_admit_tip_ok = ff_cont.as_ref().is_some_and(|cont| {
            cont.jump_snap
                .as_ref()
                .is_some_and(|s| s.pc > 0 && s.gas_remaining > 0 && !s.stack.is_empty())
        });
        let suffix_jump_would = ordered_admit_snap_jump_env
            && suffix_jump_eligible
            && read_prefix_ok
            && ordered_admit_depth_ok
            && ordered_admit_memory_ok
            && ordered_admit_tip_ok;
        // Dig arm only when JUMP env set; production keeps aj=0 / SoftWait Soft=0.
        let mut suffix_jump = suffix_jump_would;
        let _ = suffix_jump_eligible;
        // Iter22: OrderedAdmit abs jump only behind fully Validated prefix (all tx < me).
        // Hang-free yield spin; refuse jump if prefix not ready — eliminates MV races
        // that made ERC-20 aj>0 ∧ seq≠par under concurrency. Dig-only (suffix_jump
        // already env-gated). No BO park.
        // Iter25: tip_sloads skip of this spin falsified on Lean p2 (seq≠par) —
        // keep full prefix Validated for silent-default ResumePath safety.
        // Iter26: Validated-fresh FF-path tips still require this same window.
        if suffix_jump {
            let me = tx_version.tx_idx;
            let mut prefix_ok = me == 0;
            if !prefix_ok {
                // Iter27: deeper all-prefix Validated spin under 597 fan-out
                // (not tip_sloads skip — same gate, more yield budget).
                for _ in 0..32768 {
                    let mut all_v = true;
                    for i in 0..me {
                        if !self.specfence.scheduler.is_validated(i) {
                            all_v = false;
                            break;
                        }
                    }
                    if all_v {
                        prefix_ok = true;
                        break;
                    }
                    std::thread::yield_now();
                }
            }
            if !prefix_ok {
                suffix_jump = false;
                self.specfence.metrics.record_absolute_jump_fallback();
                if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
                    eprintln!("JUMP_DIG prefix_timeout me={me}");
                }
            }
        }
        let live_prime = false;
        let _ = live_prime;
        let capture_window = false;
        let _ = memory_lite_ok; // Iter8 path retained; OrderedAdmit-snap uses read_prefix_ok

        let journal_stream = self
            .specfence
            .finegrain
            .is_some_and(|fg| fg.journal_enabled());
        let research_inspect = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && !lean
            && crate::specfence::research_inspect_enabled();
        let use_inspect = journal_stream || research_inspect;

        // Iter20–22: Validated-safe origin seed for OrderedAdmit-snap PC skip. Stale FF
        // origins (writer reincarnated / Estimate) caused SoftWait/InconsistentRead
        // livelock under concurrency — refuse jump rather than seed Estimate.
        // Iter21: matching MvMemory origins must be Validated.
        // Iter22: (1) matching origins also value-check against MV Data;
        // (2) defer read_set install until after successful apply_to_interp
        // via arm_ff_origin_seeds (pre-seed + failed apply poisoned ERC-20 seq≠par);
        // (3) FF journal warm on apply (EIP-2929). research_inspect keeps classic seed.
        if rewind_resume && suffix_jump {
            let mut seeds: Vec<(MemoryLocationHash, ReadOrigin)> = Vec::new();
            let mut all_safe = true;
            for (location_hash, ff) in self.specfence.partial_retry.ff_values(tx_version.tx_idx) {
                let (origin, is_storage) = match &ff {
                    FfValue::Storage { origin, .. } => (*origin, true),
                    FfValue::Basic { origin, .. } => (*origin, false),
                };
                let current = self
                    .mv_memory
                    .last_data_before(location_hash, tx_version.tx_idx)
                    .map(|(tx_idx, tx_incarnation)| (tx_idx, tx_incarnation));
                let read_origin = if current == origin {
                    match origin {
                        Some((tx_idx, tx_incarnation)) => {
                            // Iter21 hang fix: matching origin must be Validated.
                            if !self.specfence.scheduler.is_validated(tx_idx) {
                                all_safe = false;
                                break;
                            }
                            // Iter22: matching incarnation must still publish FF value
                            // (defense vs aborted/replaced Data under concurrency).
                            let written = self.mv_memory.data.get(&location_hash);
                            let value_ok = match (is_storage, &ff, written.as_ref()) {
                                (true, FfValue::Storage { value, .. }, Some(map)) => matches!(
                                    map.get(&tx_idx),
                                    Some(MemoryEntry::Data(inc, MemoryValue::Storage(v)))
                                        if *inc == tx_incarnation && v == value
                                ),
                                (false, FfValue::Basic { basic, .. }, Some(map)) => match map
                                    .get(&tx_idx)
                                {
                                    Some(MemoryEntry::Data(inc, MemoryValue::Basic(cur_b)))
                                        if *inc == tx_incarnation =>
                                    {
                                        cur_b.balance == basic.balance && cur_b.nonce == basic.nonce
                                    }
                                    _ => false,
                                },
                                _ => false,
                            };
                            if !value_ok {
                                all_safe = false;
                                break;
                            }
                            ReadOrigin::MvMemory(TxVersion {
                                tx_idx,
                                tx_incarnation,
                            })
                        }
                        None => ReadOrigin::Storage,
                    }
                } else if let Some((w_idx, w_inc)) = current {
                    // Origin bumped — only Validated + value-stable may rebind.
                    if !self.specfence.scheduler.is_validated(w_idx) {
                        all_safe = false;
                        break;
                    }
                    let written = self.mv_memory.data.get(&location_hash);
                    let value_ok = match (is_storage, &ff, written.as_ref()) {
                        (true, FfValue::Storage { value, .. }, Some(map)) => matches!(
                            map.get(&w_idx),
                            Some(MemoryEntry::Data(inc, MemoryValue::Storage(v)))
                                if *inc == w_inc && v == value
                        ),
                        (false, FfValue::Basic { basic, .. }, Some(map)) => match map.get(&w_idx) {
                            // Match try_ff_basic Validated value-stable (balance+nonce).
                            Some(MemoryEntry::Data(inc, MemoryValue::Basic(cur_b)))
                                if *inc == w_inc =>
                            {
                                cur_b.balance == basic.balance && cur_b.nonce == basic.nonce
                            }
                            _ => false,
                        },
                        _ => false,
                    };
                    if !value_ok {
                        all_safe = false;
                        break;
                    }
                    ReadOrigin::MvMemory(TxVersion {
                        tx_idx: w_idx,
                        tx_incarnation: w_inc,
                    })
                } else if origin.is_none() {
                    ReadOrigin::Storage
                } else {
                    // FF origin points at missing/Estimate writer — refuse jump.
                    all_safe = false;
                    break;
                };
                seeds.push((location_hash, read_origin));
            }
            if all_safe {
                // Iter22: pre-seed read_set (needed if any mid-exec path observes origins)
                // AND stash copy for clear-on-failed-apply to avoid poison.
                arm_ff_origin_seeds(seeds.clone());
                let db = self.evm.ctx().db_mut();
                for (location_hash, read_origin) in seeds {
                    let origins = db.read_set.entry(location_hash).or_default();
                    if origins.is_empty() {
                        origins.push(read_origin);
                    }
                }
            } else {
                suffix_jump = false;
                self.specfence.metrics.record_absolute_jump_fallback();
                if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
                    eprintln!("JUMP_DIG origin_unsafe tx={}", tx_version.tx_idx);
                }
            }
        } else if rewind_resume && research_inspect {
            let db = self.evm.ctx().db_mut();
            for (location_hash, ff) in self.specfence.partial_retry.ff_values(tx_version.tx_idx) {
                let origin = match &ff {
                    FfValue::Storage { origin, .. } | FfValue::Basic { origin, .. } => *origin,
                };
                let read_origin = match origin {
                    Some((tx_idx, tx_incarnation)) => ReadOrigin::MvMemory(TxVersion {
                        tx_idx,
                        tx_incarnation,
                    }),
                    None => ReadOrigin::Storage,
                };
                db.read_set
                    .entry(location_hash)
                    .or_default()
                    .push(read_origin);
            }
        }
        let profile = crate::specfence::profile_timing_enabled();
        let handler_t0 = profile.then(Instant::now);
        // Iter19: read-prefix OrderedAdmit-snap jump arms WITHOUT protocol TLS (no WaitHard
        // demote / SSTORE inspect). Protocol TLS only for research inspect / capture_window.
        let protocol_handler = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && (use_inspect || capture_window);
        // Iter24/25: OrderedAdmit-snap TLS — Mass (=1 dig) on every Lean execute; ResumePath
        // (silent default) only on SuffixRepair resume / force_ordered_admit /
        // needs_live_capture — no mass-path SNAP tax on every Handler run.
        let snap_mode = ordered_admit_snap_mode();
        // Iter24/25/26: ResumePath capture on SuffixRepair resume / force_ordered_admit /
        // needs_live_capture — discovery (inc=0) SNAP-free. Broad inc>0 capture
        // falsified Iter25. Iter26 arms tip≡FF via try_ff_storage + Validated
        // OrderedAdmit-on-Data only. SoftWait Soft=0.
        let repair_capture = rewind_resume
            || needs_capture
            || (tx_version.tx_incarnation > 0
                && self
                    .specfence
                    .partial_retry
                    .has_force_ordered_admit(tx_version.tx_idx));
        let use_ordered_admit_snap = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && lean
            && match snap_mode {
                OrderedAdmitSnapMode::Off => false,
                OrderedAdmitSnapMode::Mass => true,
                OrderedAdmitSnapMode::ResumePath => repair_capture,
            };
        let run_result = if protocol_handler {
            let partial_retry = self.specfence.partial_retry;
            let metrics = self.specfence.metrics;
            let tx_idx = tx_version.tx_idx;
            let incarnation = tx_version.tx_incarnation;
            let fg = self.specfence.finegrain.filter(|f| f.journal_enabled());
            let gas_limit = Some(tx.gas_limit);
            with_protocol_tls_journal(
                tx_idx,
                incarnation,
                fg,
                gas_limit,
                partial_retry,
                metrics,
                || {
                    // Research inspect jump arm (Iter8 path).
                    let plant_jump = research_inspect && rewind_resume;
                    if plant_jump {
                        let jumped = partial_retry.ff_continuation(tx_idx).is_some_and(|cont| {
                            try_arm_safe_absolute_jump(tx_idx, partial_retry, &cont, metrics)
                        });
                        if !jumped {
                            if let Some(cont) = partial_retry.ff_continuation(tx_idx) {
                                if !cont.call_outcomes.is_empty() {
                                    arm_call_outcome_cache(cont.call_outcomes);
                                }
                            }
                            if let Some(snap) = partial_retry.ff_boundary(tx_idx) {
                                if snap.opcode_steps > 0 {
                                    metrics.record_pc_resume(snap.opcode_steps);
                                }
                            } else {
                                let n = partial_retry.ff_entries(tx_idx) as u64;
                                if n > 0 {
                                    metrics.record_pc_resume(n);
                                }
                            }
                        }
                    }
                    let result = self.chain.run_pevm_tx(&mut self.evm, use_inspect);
                    if plant_jump {
                        partial_retry.note_jump_applied(tx_idx, resume_was_applied());
                    }
                    // Consume one-shot capture flag after non-Blocking run.
                    if needs_capture {
                        let blocked =
                            matches!(&result, Err(EVMError::Database(ReadError::Blocking(_))));
                        if !blocked {
                            let _ = partial_retry.take_needs_live_capture(tx_idx);
                        }
                    }
                    if use_inspect {
                        let steps = steps_this_run();
                        metrics.record_inspector_steps(steps, rewind_resume);
                    }
                    result
                },
            )
        } else {
            let partial_retry = self.specfence.partial_retry;
            let metrics = self.specfence.metrics;
            let tx_idx = tx_version.tx_idx;
            let mut run_body = || {
                // Iter20: arm OrderedAdmit-snap read-prefix absolute jump hang-free when
                // Validated-safe seed passed (suffix_jump). Handler run_exec_loop
                // applies PENDING_RESUME; no protocol TLS / inspect_run.
                let mut did_jump = false;
                if suffix_jump && rewind_resume {
                    did_jump = partial_retry.ff_continuation(tx_idx).is_some_and(|cont| {
                        let ok = try_arm_safe_absolute_jump_gated(
                            tx_idx,
                            partial_retry,
                            &cont,
                            metrics,
                            true,
                        );
                        if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
                            eprintln!(
                                "JUMP_DIG try_arm ok={ok} refuse={} tip_sloads={} steps={}",
                                jump_refuse_reason(&cont),
                                cont.jump_snap
                                    .as_ref()
                                    .map(|s| s.tip_sloads.len())
                                    .unwrap_or(0),
                                cont.jump_snap.as_ref().map(|s| s.opcode_steps).unwrap_or(0),
                            );
                        }
                        ok
                    });
                    if !did_jump {
                        // Armed seeds only meaningful if jump arm succeeded.
                        let _ = take_ff_origin_seeds();
                        if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
                            eprintln!("JUMP_DIG arm_or_cont_miss tx={tx_idx}");
                        }
                    }
                }
                // Hang-free credit consume when OrderedAdmit tip exists but jump not armed
                // (unsafe origins / jump_is_safe refuse / JUMP env off with SNAP on).
                if rewind_resume && read_prefix_ok && !did_jump {
                    if let Some(cont) = partial_retry.ff_continuation(tx_idx) {
                        if let Some(snap) = cont.jump_snap.as_ref() {
                            if snap.opcode_steps > 0 {
                                metrics.record_ordered_admit_snap_credit(snap.opcode_steps);
                            }
                        }
                        // Iter27 dig: classify why suffix_jump stayed false / arm refused.
                        if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
                            use std::sync::atomic::{AtomicUsize, Ordering as Ord};
                            static DIG_CREDIT: AtomicUsize = AtomicUsize::new(0);
                            static DIG_SAFE: AtomicUsize = AtomicUsize::new(0);
                            static DIG_DEPTH: AtomicUsize = AtomicUsize::new(0);
                            static DIG_DEPTH_GT1: AtomicUsize = AtomicUsize::new(0);
                            static DIG_MEM: AtomicUsize = AtomicUsize::new(0);
                            static DIG_TIP: AtomicUsize = AtomicUsize::new(0);
                            static DIG_ELIG: AtomicUsize = AtomicUsize::new(0);
                            static DIG_WOULD: AtomicUsize = AtomicUsize::new(0);
                            static DIG_ENV: AtomicUsize = AtomicUsize::new(0);
                            DIG_CREDIT.fetch_add(1, Ord::Relaxed);
                            let safe = jump_is_safe(&cont);
                            if safe {
                                DIG_SAFE.fetch_add(1, Ord::Relaxed);
                            }
                            let depth =
                                cont.jump_snap.as_ref().map(|s| s.call_depth).unwrap_or(999);
                            if depth <= 1 {
                                DIG_DEPTH.fetch_add(1, Ord::Relaxed);
                            }
                            if depth > 1 {
                                DIG_DEPTH_GT1.fetch_add(1, Ord::Relaxed);
                            }
                            let mem_ok = cont
                                .jump_snap
                                .as_ref()
                                .is_some_and(|s| s.memory_words == 0 || !s.memory.is_empty());
                            if mem_ok {
                                DIG_MEM.fetch_add(1, Ord::Relaxed);
                            }
                            let tip_ok = cont.jump_snap.as_ref().is_some_and(|s| {
                                s.pc > 0 && s.gas_remaining > 0 && !s.stack.is_empty()
                            });
                            if tip_ok {
                                DIG_TIP.fetch_add(1, Ord::Relaxed);
                            }
                            if suffix_jump_eligible {
                                DIG_ELIG.fetch_add(1, Ord::Relaxed);
                            }
                            if suffix_jump_would {
                                DIG_WOULD.fetch_add(1, Ord::Relaxed);
                            }
                            if ordered_admit_snap_jump_env {
                                DIG_ENV.fetch_add(1, Ord::Relaxed);
                            }
                            let reason = jump_refuse_reason(&cont);
                            let n = DIG_CREDIT.load(Ord::Relaxed);
                            if n <= 8 || n % 32 == 0 {
                                eprintln!(
                                    "JUMP_DIG credit=#{n} safe={safe} depth={depth} mem={mem_ok} tip={tip_ok} elig={} would={} env={} refuse={reason} tip_sloads={} steps={}",
                                    suffix_jump_eligible,
                                    suffix_jump_would,
                                    ordered_admit_snap_jump_env,
                                    cont.jump_snap
                                        .as_ref()
                                        .map(|s| s.tip_sloads.len())
                                        .unwrap_or(0),
                                    cont.jump_snap.as_ref().map(|s| s.opcode_steps).unwrap_or(0),
                                );
                            }
                            // Periodically dump totals
                            if n == 1 || n % 64 == 0 {
                                eprintln!(
                                    "JUMP_DIG totals credit={} safe={} depth<=1={} depth>1={} mem={} tip={} elig={} would={} env={}",
                                    DIG_CREDIT.load(Ord::Relaxed),
                                    DIG_SAFE.load(Ord::Relaxed),
                                    DIG_DEPTH.load(Ord::Relaxed),
                                    DIG_DEPTH_GT1.load(Ord::Relaxed),
                                    DIG_MEM.load(Ord::Relaxed),
                                    DIG_TIP.load(Ord::Relaxed),
                                    DIG_ELIG.load(Ord::Relaxed),
                                    DIG_WOULD.load(Ord::Relaxed),
                                    DIG_ENV.load(Ord::Relaxed),
                                );
                            }
                        } else {
                            let _ = (
                                ordered_admit_snap_jump_env,
                                jump_is_safe(&cont),
                                jump_refuse_reason(&cont),
                            );
                        }
                    }
                }
                let result = self.chain.run_pevm_tx(
                    &mut self.evm,
                    use_inspect && self.specfence.mode == crate::ConcurrencyMode::SpecFence,
                );
                if did_jump {
                    let applied = resume_was_applied();
                    let seeds = take_ff_origin_seeds();
                    if !applied {
                        // Iter22: failed PC apply — drop pre-seeded FF origins so cold
                        // re-exec / natural reads own the read_set (avoid poison).
                        if !seeds.is_empty() {
                            let db = self.evm.ctx().db_mut();
                            for (location_hash, seeded) in &seeds {
                                if let Some(origins) = db.read_set.get_mut(location_hash) {
                                    if origins.last() == Some(seeded) && origins.len() == 1 {
                                        db.read_set.remove(location_hash);
                                    }
                                }
                            }
                        }
                    }
                    partial_retry.note_jump_applied(tx_idx, applied);
                }
                result
            };
            if use_ordered_admit_snap {
                with_ordered_admit_snap_tls(tx_idx, partial_retry, metrics, run_body)
            } else {
                run_body()
            }
        };
        if let Some(t0) = handler_t0 {
            self.specfence
                .metrics
                .add_profile_handler_ns(t0.elapsed().as_nanos() as u64);
        }
        // Iter2: consume live_prime only after inspect completed (keep on Blocking).
        if live_prime {
            let blocked = matches!(&run_result, Err(EVMError::Database(ReadError::Blocking(_))));
            if !blocked {
                let _ = self
                    .specfence
                    .partial_retry
                    .take_needs_live_capture(tx_version.tx_idx);
            }
        }

        match run_result {
            Ok(exec_result) => {
                // M1f: jumped Success may commit when jump_is_safe. Validation abort
                // still disables further jumps via pevm.rs circuit breaker (anti-livelock).

                // There are at least six locations most of the time: the sender,
                // the recipient, and up to four fee recipients (beneficiary, base fee,
                // L1 fee, operator fee on OP Stack chains).
                let mut write_set = WriteSet::with_capacity(6);

                let ctx = self.evm.ctx();
                let state = ctx.journal_mut().finalize();

                for (address, account) in &state {
                    if account.is_selfdestructed() {
                        // TODO: Also write [SelfDestructed] to the basic location?
                        // For now we are betting on [code_hash] triggering the sequential
                        // fallback when we read a self-destructed contract.
                        write_set.push((
                            hash_deterministic(MemoryLocation::CodeHash(*address)),
                            MemoryValue::SelfDestructed,
                        ));
                        continue;
                    }

                    if account.is_touched() {
                        let account_location_hash =
                            hash_deterministic(MemoryLocation::Basic(*address));
                        let read_account = ctx.db().read_accounts.get(&account_location_hash);

                        let has_code = !account.info.is_empty_code_hash();
                        let is_new_code = has_code
                            && read_account.is_none_or(|(_, code_hash)| code_hash.is_none());

                        // Write new account changes
                        if is_new_code
                            || read_account.is_none()
                            || read_account.is_some_and(|(basic, _)| {
                                basic.nonce != account.info.nonce
                                    || basic.balance != account.info.balance
                            })
                        {
                            if ctx.db().is_lazy {
                                if account_location_hash == from_hash {
                                    write_set.push((
                                        account_location_hash,
                                        MemoryValue::LazySender(U256::MAX - account.info.balance),
                                    ));
                                } else if Some(account_location_hash) == to_hash {
                                    write_set.push((
                                        account_location_hash,
                                        MemoryValue::LazyRecipient(tx.value),
                                    ));
                                }
                            }
                            // We don't register empty accounts after [SPURIOUS_DRAGON]
                            // as they are cleared. This can only happen via 2 ways:
                            // 1. Self-destruction which is handled by an if above.
                            // 2. Sending 0 ETH to an empty account, which we treat as a
                            // non-write here. A later read would trace back to storage
                            // and return a [None], i.e., [LoadedAsNotExisting]. Without
                            // this check it would write then read a [Some] default
                            // account, which may yield a wrong gas fee, etc.
                            else if !self.is_eip_161_enabled || !account.is_empty() {
                                write_set.push((
                                    account_location_hash,
                                    MemoryValue::Basic(AccountBasic {
                                        balance: account.info.balance,
                                        nonce: account.info.nonce,
                                    }),
                                ));
                            }
                        }

                        // Write new contract
                        if is_new_code {
                            write_set.push((
                                hash_deterministic(MemoryLocation::CodeHash(*address)),
                                MemoryValue::CodeHash(account.info.code_hash),
                            ));
                            self.mv_memory
                                .new_bytecodes
                                .entry(account.info.code_hash)
                                .or_insert_with(|| account.info.code.clone().unwrap());
                        }
                    }

                    // TODO: We should move this changed check to our read set like for account info?
                    for (slot, value) in account.changed_storage_slots() {
                        let loc = hash_deterministic(MemoryLocation::Storage(*address, *slot));
                        write_set.push((loc, MemoryValue::Storage(value.present_value)));
                        // M1i: capture storage presents for RewindTo residual republish
                        // + absolute-jump journal slot replay (never journal-blob poison).
                        // gas_remaining_after filled from Inspector post-SSTORE captures.
                        if self.specfence.mode == crate::ConcurrencyMode::SpecFence
                            && self.specfence.certificates.rem_legal(tx_version.tx_idx)
                        {
                            self.specfence.partial_retry.note_write_replay(
                                tx_version.tx_idx,
                                loc,
                                StorageWriteReplay {
                                    address: *address,
                                    slot: *slot,
                                    original: value.original_value,
                                    present: value.present_value,
                                    gas_remaining_after: 0, // filled via post_sstore_gases
                                },
                            );
                        }
                    }
                }

                // M1d/M1e/M1h: when live PC jump omitted prefix SSTORE/account writes from
                // the revm journal, re-publish certified-prefix writes so record() does
                // not drop them — from MvMemory residual Data and/or SpecFence
                // write_replays (never journal-blob present_values).
                // Lean journal-FF-only Handler::run re-executes prefix stores — residual
                // republish would paste stale MvMemory Data and break seq≡par.
                if rewind_resume && (suffix_jump || research_inspect) {
                    let suffix: hashbrown::HashSet<_, BuildIdentityHasher> = self
                        .specfence
                        .partial_retry
                        .ff_suffix_writes(tx_version.tx_idx)
                        .into_iter()
                        .collect();
                    for loc in self.mv_memory.residual_writes(tx_version.tx_idx) {
                        if suffix.contains(&loc) {
                            continue;
                        }
                        if write_set.iter().any(|(l, _)| *l == loc) {
                            continue;
                        }
                        if let Some(value) =
                            self.mv_memory.published_data_value(tx_version.tx_idx, loc)
                        {
                            write_set.push((loc, value));
                        }
                    }
                    // M1h: SpecFence-captured write presents (source of truth when
                    // residual was ESTIMATEd or jump skipped SSTORE in revm journal).
                    if let Some(cont) = self
                        .specfence
                        .partial_retry
                        .ff_continuation(tx_version.tx_idx)
                    {
                        for wr in &cont.write_replays {
                            let loc =
                                hash_deterministic(MemoryLocation::Storage(wr.address, wr.slot));
                            if suffix.contains(&loc) {
                                continue;
                            }
                            if write_set.iter().any(|(l, _)| *l == loc) {
                                continue;
                            }
                            write_set.push((loc, MemoryValue::Storage(wr.present)));
                        }
                    }
                }

                // Rewards
                let mut gas_price = if let Some(priority_fee) = tx.gas_priority_fee {
                    std::cmp::min(
                        tx.gas_price,
                        priority_fee.saturating_add(self.block_env.basefee as u128),
                    )
                } else {
                    tx.gas_price
                };
                if self.is_eip_161_enabled {
                    gas_price = gas_price.saturating_sub(self.block_env.basefee as u128);
                }
                let rewards = self.chain.get_rewards(
                    self.beneficiary_location_hash,
                    U256::from(exec_result.tx_gas_used()),
                    U256::from(gas_price),
                    self.block_env.basefee,
                    full_tx,
                );
                for (recipient, amount) in rewards {
                    if let Some((_, value)) = write_set
                        .iter_mut()
                        .find(|(location, _)| location == &recipient)
                    {
                        match value {
                            MemoryValue::Basic(basic) => {
                                basic.balance = basic.balance.saturating_add(amount)
                            }
                            MemoryValue::LazySender(subtraction) => {
                                *subtraction = subtraction.saturating_sub(amount)
                            }
                            MemoryValue::LazyRecipient(addition) => {
                                *addition = addition.saturating_add(amount)
                            }
                            _ => return Err(ReadError::InvalidMemoryValueType.into()),
                        }
                    } else {
                        write_set.push((recipient, MemoryValue::LazyRecipient(amount)));
                    }
                }

                let (is_lazy, optimistic_majority_lazy, read_set, sf_occ_shaped) = {
                    let db = ctx.db_mut();
                    db.flush_access_census();
                    (
                        db.is_lazy,
                        db.optimistic_majority_lazy,
                        std::mem::take(&mut db.read_set),
                        db.sf_occ_shaped,
                    )
                };

                if is_lazy {
                    self.mv_memory
                        .add_lazy_addresses([tx.caller, *tx.kind.to().unwrap()]);
                }

                let mut flags = if tx_version.tx_idx > 0 && !is_lazy {
                    FinishExecFlags::NeedValidation
                } else {
                    FinishExecFlags::empty()
                };

                let optimistic_ungated = self.specfence.mode == crate::ConcurrencyMode::SpecFence
                    && self
                        .specfence
                        .policy
                        .is_some_and(|p| p.skip_ungated_tx_path_tax())
                    && !self.specfence.ready_edges.is_gated(tx_version.tx_idx);
                if !optimistic_ungated
                    && self.specfence.mode == crate::ConcurrencyMode::SpecFence
                    && self.specfence.certificates.rem_legal(tx_version.tx_idx)
                {
                    for (loc, value) in &write_set {
                        // G2: plant Write ordinal always (HotSet not required).
                        self.specfence.partial_retry.note_access(
                            tx_version.tx_idx,
                            *loc,
                            AccessMode::Write,
                        );
                        // G4: Publish → ordered_admit prior credit.
                        self.specfence.learner.note_publish(*loc);
                        // A2: first confirmed wr/publish → immediate Avoid broadcast.
                        if self
                            .specfence
                            .sketch
                            .broadcast_avoid(*loc, tx_version.tx_idx)
                        {
                            self.specfence.edges.broadcast_avoid(*loc);
                            self.specfence.metrics.record_avoid_broadcast();
                        }
                        // First-wave serial-lane: abort-derived k template only.
                        // Publish never plants PredictedEssential from Detect last_k
                        // (that OrderedAdmit-taxed quiet / ¬PredictedEssential accesses).
                        let k_tmpl = self.specfence.learner.dominant_k(*loc);
                        if k_tmpl > 0 {
                            self.specfence.sketch.mark_access_class(*loc, k_tmpl);
                        }
                        self.specfence.process.note_avoid(*loc);
                        let kind = match value {
                            MemoryValue::Basic(_)
                            | MemoryValue::LazySender(_)
                            | MemoryValue::LazyRecipient(_)
                            | MemoryValue::SelfDestructed => CheckpointKind::AccountWrite,
                            MemoryValue::Storage(_) => CheckpointKind::StorageWrite,
                            // Code-hash / other account-adjacent writes.
                            _ => CheckpointKind::AccountWrite,
                        };
                        let _ = self
                            .specfence
                            .partial_retry
                            .push_checkpoint(tx_version.tx_idx, kind);
                    }
                    let _ = self
                        .specfence
                        .partial_retry
                        .push_checkpoint(tx_version.tx_idx, CheckpointKind::CallExit);
                    // G1: persist k + tx_gas_used for next-incarnation effect-depth proxy.
                    self.specfence
                        .partial_retry
                        .note_incarnation_finish(tx_version.tx_idx, exec_result.tx_gas_used());
                }

                // L4/P3: A0 ungated records MV only. Mid-execute ReadyEdge
                // insert races seq≡par (iter9 / mixed SIGSEGV). C1/L2 seed
                // at begin (hint / prior) or on the next block after promote.
                if optimistic_ungated {
                    // Light tip publish before record moves write_set (Released /
                    // ChainSpine so WaitOnce Avoid hits without spin/museum).
                    // OCC-shaped non-producer: skip tip DashMap + promote museum
                    // (install_version_tip already skipped at execute start).
                    let shaped_skip_tip = sf_occ_shaped
                        && !self
                            .specfence
                            .access_arms
                            .is_wait_once_producer(tx_version.tx_idx);
                    let tip_locs: Vec<_> = if shaped_skip_tip {
                        Vec::new()
                    } else {
                        write_set
                            .iter()
                            .map(|(loc, _)| *loc)
                            .filter(|&loc| {
                                self.specfence.sf_tips.is_chain_loc(loc)
                                    || (self.specfence.scheduler.block_size()
                                        <= crate::specfence::THIN_SHELL_N
                                        && (self.specfence.access_arms.is_crit_loc(loc)
                                            || self.specfence.access_arms.is_wait_once(loc)))
                            })
                            .collect()
                    };
                    let (wrote_new_location, contended) =
                        self.mv_memory.record(tx_version, read_set, write_set);
                    for loc in tip_locs {
                        let exact = self.specfence.sf_tips.publish_data(
                            loc,
                            tx_version.tx_idx,
                            tx_version.tx_incarnation,
                        );
                        // ChainSpine one-hop: schedule wake planted succ + exact
                        // waiters into Q_released (Avoid on Released, not park).
                        if self.specfence.sf_tips.is_chain_loc(loc) {
                            for c in exact {
                                self.specfence.wave.push_ready(c);
                            }
                            self.specfence.ready_edges.wake_planted_on_publish(
                                tx_version.tx_idx,
                                self.specfence.wave,
                            );
                        }
                    }
                    if wrote_new_location {
                        flags |= FinishExecFlags::WroteNewLocation;
                    }
                    if !shaped_skip_tip {
                        self.specfence
                            .metrics
                            .record_speculate(tx.caller, tx.kind.to().copied());
                        for loc in contended {
                            if loc == self.beneficiary_location_hash {
                                continue;
                            }
                            let addr = if loc == from_hash {
                                Some(tx.caller)
                            } else if Some(loc) == to_hash {
                                tx.kind.to().copied()
                            } else {
                                None
                            };
                            if addr == Some(self.specfence.beneficiary) {
                                continue;
                            }
                            self.promote_region(loc, addr);
                        }
                    }
                    let receipt = receipt_from_revm(exec_result);
                    let state = state_transitions_from_revm(self.is_eip_161_enabled, state);
                    if let Some(slot) = result_slot {
                        slot.receipt = receipt;
                        slot.state.clear();
                        slot.state.extend(state);
                    } else {
                        *result_slot = Some(PevmTxExecutionResult {
                            receipt,
                            state: state.collect(),
                        });
                    }
                    return Ok(flags);
                }

                // R3: HotSet H_w ignores LazyRecipient multi-writer noise (popular
                // payees on wide blocks; fine-grain G* drops basic_lazy). Keep
                // LazySender so same-sender RAW storms still escalate. Storage/Basic
                // full writes always count. Still learn full WŜ for M3 prior.
                let hotset_writer_locs: Vec<MemoryLocationHash> = write_set
                    .iter()
                    .filter_map(|(loc, val)| {
                        if *loc == self.beneficiary_location_hash {
                            return None;
                        }
                        match val {
                            MemoryValue::LazyRecipient(_) => None,
                            _ => Some(*loc),
                        }
                    })
                    .collect();
                let all_write_locs: Vec<MemoryLocationHash> = write_set
                    .iter()
                    .filter_map(|(loc, _)| {
                        if *loc == self.beneficiary_location_hash {
                            None
                        } else {
                            Some(*loc)
                        }
                    })
                    .collect();
                let effective_write_locs: Vec<MemoryLocationHash> = write_set
                    .iter()
                    .filter_map(|(loc, val)| {
                        if *loc == self.beneficiary_location_hash {
                            return None;
                        }
                        match val {
                            MemoryValue::LazyRecipient(_) | MemoryValue::LazySender(_) => None,
                            _ => Some(*loc),
                        }
                    })
                    .collect();
                if let Some(fg) = self.specfence.finegrain {
                    fg.deep_register_writes(
                        tx_version.tx_idx,
                        tx_version.tx_incarnation,
                        &write_set,
                    );
                    let total_steps = if journal_stream {
                        Some(steps_this_run() as usize)
                    } else {
                        None
                    };
                    fg.deep_finish_consumer(
                        tx_version.tx_idx,
                        tx_version.tx_incarnation,
                        Some(exec_result.tx_gas_used()),
                        Some(tx.gas_limit),
                        total_steps,
                    );
                }
                if self.specfence.mode == crate::ConcurrencyMode::SpecFence
                    && let Some(p) = self.specfence.policy
                {
                    for (loc, val) in &write_set {
                        if *loc == self.beneficiary_location_hash {
                            continue;
                        }
                        match val {
                            MemoryValue::LazyRecipient(_) | MemoryValue::LazySender(_) => {
                                p.note_loc_write(*loc, true)
                            }
                            MemoryValue::Storage(_) | MemoryValue::CodeHash(_) => {
                                p.note_loc_storage(*loc)
                            }
                            _ => p.note_loc_write(*loc, false),
                        }
                    }
                }
                let (wrote_new_location, contended) =
                    self.mv_memory.record(tx_version, read_set, write_set);
                // M3: learn process WŜ from this incarnation's writes (no residual publish).
                // R1/R3: feed HotSet writer counts (H_w) from non-lazy writes only.
                let _ = optimistic_majority_lazy;
                if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                    crate::specfence::admit::admit_seed_on_write_set(
                        self.specfence.ready_edges,
                        self.specfence.hints,
                        self.specfence.wave,
                        self.specfence.policy,
                        tx_version.tx_idx,
                        tx.caller,
                        tx.kind.to().copied(),
                        &all_write_locs,
                        &effective_write_locs,
                    );
                    if self.specfence.certificates.rem_legal(tx_version.tx_idx) {
                        let locs: Vec<_> = self.mv_memory.write_locations(tx_version.tx_idx);
                        self.specfence.rw_prior.observe_write_set(&locs, None);
                        for loc in &hotset_writer_locs {
                            self.specfence.hotset.note_writer(*loc, tx_version.tx_idx);
                        }
                        // A2: progressive DAG — Data is visible; wake Blocking waiters
                        // before is_done (SoftWait Soft stays 0).
                        self.wake_on_data_publish(
                            tx_version.tx_idx,
                            tx_version.tx_incarnation,
                            &locs,
                        );
                    } else {
                        self.specfence
                            .rw_prior
                            .observe_write_set(&hotset_writer_locs, None);
                        for loc in &hotset_writer_locs {
                            self.specfence.hotset.note_writer(*loc, tx_version.tx_idx);
                        }
                        // Spec publishers of a PE ℓ still wake / Avoid — not rem-gated.
                        if self.specfence.learner.has_any_predicted() {
                            let pe_locs: Vec<_> = hotset_writer_locs
                                .iter()
                                .copied()
                                .filter(|&loc| {
                                    let k = self.specfence.learner.dominant_k(loc);
                                    self.specfence.learner.predicted_essential(loc, k.max(1))
                                })
                                .collect();
                            if !pe_locs.is_empty() {
                                for &loc in &pe_locs {
                                    if self
                                        .specfence
                                        .sketch
                                        .broadcast_avoid(loc, tx_version.tx_idx)
                                    {
                                        self.specfence.edges.broadcast_avoid(loc);
                                        self.specfence.metrics.record_avoid_broadcast();
                                    }
                                }
                                self.wake_on_data_publish(
                                    tx_version.tx_idx,
                                    tx_version.tx_incarnation,
                                    &pe_locs,
                                );
                            }
                        }
                    }
                }
                if wrote_new_location {
                    flags |= FinishExecFlags::WroteNewLocation;
                }
                if self.specfence.mode.uses_regions() {
                    let from_wait = self
                        .specfence
                        .should_wait_account(&self.mv_memory.regions, &tx.caller);
                    let to_wait = tx.kind.to().is_some_and(|to| {
                        self.specfence
                            .should_wait_account(&self.mv_memory.regions, to)
                    });
                    // Independents (no hinted predecessor) still count as Speculate
                    // even when PCC seeds their account Wait with nobody to wait for.
                    let from_has_pred = self
                        .specfence
                        .hints
                        .prev(&tx.caller, tx_version.tx_idx)
                        .is_some();
                    let to_has_pred = tx.kind.to().is_some_and(|to| {
                        self.specfence.hints.prev(to, tx_version.tx_idx).is_some()
                    });
                    if (!from_wait || !from_has_pred) && (!to_wait || !to_has_pred) {
                        self.specfence
                            .metrics
                            .record_speculate(tx.caller, tx.kind.to().copied());
                    }
                    // Wave: a second hinted writer to the same account is a WW overlap.
                    self.promote_if_multi_writer(tx.caller, Some(from_hash));
                    if let Some(to) = tx.kind.to().copied() {
                        self.promote_if_multi_writer(to, to_hash);
                    }
                    for loc in contended {
                        if loc == self.beneficiary_location_hash {
                            continue;
                        }
                        let addr = if loc == from_hash {
                            Some(tx.caller)
                        } else if Some(loc) == to_hash {
                            tx.kind.to().copied()
                        } else {
                            None
                        };
                        if addr == Some(self.specfence.beneficiary) {
                            continue;
                        }
                        self.promote_region(loc, addr);
                    }
                }

                let receipt = receipt_from_revm(exec_result);
                let state = state_transitions_from_revm(self.is_eip_161_enabled, state);
                if let Some(slot) = result_slot {
                    slot.receipt = receipt;
                    slot.state.clear();
                    slot.state.extend(state);
                } else {
                    *result_slot = Some(PevmTxExecutionResult {
                        receipt,
                        state: state.collect(),
                    });
                }
                Ok(flags)
            }
            Err(EVMError::Database(read_error)) => {
                self.evm.ctx().db_mut().flush_access_census();
                Err(VmExecutionError::from(read_error))
            }
            Err(err) => {
                self.evm.ctx().db_mut().flush_access_census();
                // Optimistically retry in case some previous internal transactions send
                // more fund to the sender but hasn't been executed yet.
                // TODO: Let users define this behaviour through a mode enum or something.
                // Since this retry is safe for syncing canonical blocks but can deadlock
                // on new or faulty blocks. We can skip the transaction for new blocks and
                // error out after a number of tries for the latter.
                if tx_version.tx_idx > 0
                    && matches!(
                        err,
                        EVMError::Transaction(
                            InvalidTransaction::LackOfFundForMaxFee { .. }
                                | InvalidTransaction::NonceTooHigh { .. }
                        )
                    )
                {
                    let pred_done = self.specfence.scheduler.is_done(tx_version.tx_idx - 1);
                    let pred_passed = self
                        .specfence
                        .ready_edges
                        .leftover_min_skips_blocker(tx_version.tx_idx - 1);
                    if self
                        .specfence
                        .ready_edges
                        .is_live_leftover_min(tx_version.tx_idx)
                        && (pred_done || pred_passed)
                    {
                        // leftover_min + done prefix: do not ghost-park.
                        Err(VmExecutionError::Retry)
                    } else {
                        Err(VmExecutionError::Blocking(tx_version.tx_idx - 1))
                    }
                } else {
                    Err(VmExecutionError::ExecutionError(err))
                }
            }
        }
    }
}
