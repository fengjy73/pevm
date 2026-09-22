//! SfMvMemory — SpecFence-native version plane (SF-PS redesign v1).
//!
//! OCC [`MvMemory`] + [`MemoryEntry::Estimate`] stay baseline-only. SpecFence
//! installs **version tips** and reads via [`VisibilityPolicy`]; WaitOnce
//! consumes true Data publish — never Estimate Block.
//!
//! SoT: `lab/notes/specfence-sf-mvmemory-redesign-v1.md`

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

use dashmap::DashMap;

use crate::mv_memory::MvMemory;
use crate::{MemoryLocationHash, MemoryValue, TxIdx, TxIncarnation};

use super::VisibilityPolicy;

/// Chain spine tip state (sticky≥32 light plane — no DashMap).
const CHAIN_EMPTY: u8 = 0;
const CHAIN_VERSION: u8 = 1;
const CHAIN_RELEASED: u8 = 2;

/// SpecFence tip kind — **not** OCC [`crate::MemoryEntry::Estimate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SfTip {
    /// Writer claimed `ℓ` for this incarnation (early tip; value not yet Data).
    Version { incarnation: TxIncarnation },
    /// True Data is published in MvMemory for this writer/incarnation.
    Released { incarnation: TxIncarnation },
}

/// Result of a visibility-aware read.
#[derive(Debug, Clone)]
pub(crate) enum SfRead {
    /// Installed Data from writer `tx` incarnation `inc`.
    Data {
        writer: TxIdx,
        incarnation: TxIncarnation,
        value: MemoryValue,
    },
    /// Race tip is an ESTIMATE — only [`VisibilityPolicy::Opt`] may see this.
    Estimate { writer: TxIdx },
    /// No prior MV write; fall back to storage.
    Storage,
}

/// Shared SpecFence tip + exact-waiter plane (one per block execution).
/// SoT: version tip, live_writer(ℓ), exact waiters; Estimate forbidden.
/// Large sticky≥32 uses [`Self::bind_chain_spine`] (atomics) — not DashMap tips.
#[derive(Debug)]
pub(crate) struct SfTipTable {
    tips: DashMap<MemoryLocationHash, BTreeMap<TxIdx, SfTip>>,
    /// True unpublished writer of ℓ (RAW/WAW). Cleared on publish after tip.
    live_writer: DashMap<MemoryLocationHash, TxIdx>,
    /// Exact waiters keyed by `(location, writer)`.
    waiters: DashMap<(MemoryLocationHash, TxIdx), Vec<TxIdx>>,
    /// ChainSpineTip: sticky≥32 loc (`u64::MAX` = inactive).
    chain_loc: AtomicU64,
    /// Writer → Empty|Version|Released (dense Chain plane; not BTreeMap tips).
    chain_flags: DashMap<TxIdx, AtomicU8>,
    /// Highest unfinished chain writer claim (`usize::MAX` = none).
    chain_live: AtomicUsize,
    early_tip_n: AtomicUsize,
    publish_wake_n: AtomicUsize,
    wait_once_consume_n: AtomicUsize,
    /// Concurrent Detect|Avoid|Resolve path audit (per access, not a pipeline).
    detect_before_n: AtomicUsize,
    avoid_publish_n: AtomicUsize,
    resolve_after_fail_n: AtomicUsize,
    /// Four conflict classes × timely Avoid (b) vs late Resolve (c).
    raw_avoid_n: AtomicUsize,
    raw_late_n: AtomicUsize,
    war_avoid_n: AtomicUsize,
    war_late_n: AtomicUsize,
    waw_avoid_n: AtomicUsize,
    waw_late_n: AtomicUsize,
    chain_avoid_n: AtomicUsize,
    chain_late_n: AtomicUsize,
    /// Must stay 0 on Soft=0 Instant-off: SF Avoid never Blocks on Estimate.
    estimate_block_sf: AtomicUsize,
}

impl Default for SfTipTable {
    fn default() -> Self {
        Self {
            tips: DashMap::new(),
            live_writer: DashMap::new(),
            waiters: DashMap::new(),
            chain_loc: AtomicU64::new(u64::MAX),
            chain_flags: DashMap::new(),
            chain_live: AtomicUsize::new(usize::MAX),
            early_tip_n: AtomicUsize::new(0),
            publish_wake_n: AtomicUsize::new(0),
            wait_once_consume_n: AtomicUsize::new(0),
            detect_before_n: AtomicUsize::new(0),
            avoid_publish_n: AtomicUsize::new(0),
            resolve_after_fail_n: AtomicUsize::new(0),
            raw_avoid_n: AtomicUsize::new(0),
            raw_late_n: AtomicUsize::new(0),
            war_avoid_n: AtomicUsize::new(0),
            war_late_n: AtomicUsize::new(0),
            waw_avoid_n: AtomicUsize::new(0),
            waw_late_n: AtomicUsize::new(0),
            chain_avoid_n: AtomicUsize::new(0),
            chain_late_n: AtomicUsize::new(0),
            estimate_block_sf: AtomicUsize::new(0),
        }
    }
}

impl SfTipTable {
    #[inline]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Early version tip on the SpecFence write path (prior WS / known ℓ).
    /// Does **not** install OCC Estimate into MvMemory.
    /// Sets live_writer(ℓ) so WaitOnce can Detect without Estimate.
    /// Skips no-op reinstall of the same Version incarnation (thin tax cut).
    pub(crate) fn install_version_tip(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
        incarnation: TxIncarnation,
    ) {
        // ChainSpineTip owns sticky≥32 crit — never DashMap that ℓ.
        if self.is_chain_loc(location) {
            self.chain_claim(writer, incarnation);
            return;
        }
        {
            let mut map = self.tips.entry(location).or_default();
            match map.get(&writer) {
                Some(SfTip::Released { incarnation: inc }) if *inc >= incarnation => {
                    return;
                }
                Some(SfTip::Version { incarnation: inc }) if *inc == incarnation => {
                    // Already claimed this incarnation — no DashMap rewrite / census.
                    return;
                }
                _ => {
                    map.insert(writer, SfTip::Version { incarnation });
                    self.early_tip_n.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        // live_writer: keep the latest (highest) unfinished writer claim.
        self.live_writer
            .entry(location)
            .and_modify(|w| {
                if writer >= *w {
                    *w = writer;
                }
            })
            .or_insert(writer);
    }

    /// SoT publish order: tip Released → clear live_writer → wake exact waiters.
    pub(crate) fn publish_data(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
        incarnation: TxIncarnation,
    ) -> Vec<TxIdx> {
        if self.is_chain_loc(location) {
            self.chain_release(writer, incarnation);
            return self.wake_exact(location, writer);
        }
        {
            let mut map = self.tips.entry(location).or_default();
            map.insert(writer, SfTip::Released { incarnation });
        }
        // Clear live_writer only when this writer still owns the claim.
        let _ = self
            .live_writer
            .remove_if(&location, |_, w| *w == writer);
        self.wake_exact(location, writer)
    }

    /// Bind sticky≥32 ChainSpineTip (light flags, not BTreeMap tips).
    pub(crate) fn bind_chain_spine(&self, loc: MemoryLocationHash, writers: &[TxIdx]) {
        self.chain_loc.store(loc, Ordering::Relaxed);
        self.chain_flags.clear();
        for &w in writers {
            self.chain_flags.insert(w, AtomicU8::new(CHAIN_EMPTY));
        }
        self.chain_live.store(usize::MAX, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn is_chain_loc(&self, loc: MemoryLocationHash) -> bool {
        let c = self.chain_loc.load(Ordering::Relaxed);
        c != u64::MAX && c == loc
    }

    /// Claim Version on ChainSpineTip.
    pub(crate) fn chain_claim(&self, writer: TxIdx, _incarnation: TxIncarnation) {
        let Some(slot) = self.chain_flags.get(&writer) else {
            return;
        };
        let prev = slot.swap(CHAIN_VERSION, Ordering::Release);
        if prev == CHAIN_EMPTY {
            self.early_tip_n.fetch_add(1, Ordering::Relaxed);
        }
        let _ = self.chain_live.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
            if cur == usize::MAX || writer >= cur {
                Some(writer)
            } else {
                None
            }
        });
    }

    /// Released on ChainSpineTip after true Data publish.
    pub(crate) fn chain_release(&self, writer: TxIdx, _incarnation: TxIncarnation) {
        if let Some(slot) = self.chain_flags.get(&writer) {
            slot.store(CHAIN_RELEASED, Ordering::Release);
        }
        let _ = self
            .chain_live
            .compare_exchange(writer, usize::MAX, Ordering::Relaxed, Ordering::Relaxed);
    }

    /// Clear chain claim on abort (stale Version must not park successors).
    pub(crate) fn chain_clear(&self, writer: TxIdx) {
        if let Some(slot) = self.chain_flags.get(&writer) {
            slot.store(CHAIN_EMPTY, Ordering::Release);
        }
        let _ = self
            .chain_live
            .compare_exchange(writer, usize::MAX, Ordering::Relaxed, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn chain_released(&self, writer: TxIdx) -> bool {
        self.chain_flags
            .get(&writer)
            .is_some_and(|s| s.load(Ordering::Acquire) == CHAIN_RELEASED)
    }

    #[inline]
    pub(crate) fn chain_version_or_released(&self, writer: TxIdx) -> bool {
        self.chain_flags.get(&writer).is_some_and(|s| {
            matches!(
                s.load(Ordering::Acquire),
                CHAIN_VERSION | CHAIN_RELEASED
            )
        })
    }

    /// True unpublished writer of ℓ (structure RAW/WAW), if any.
    #[inline]
    pub(crate) fn live_writer(&self, location: MemoryLocationHash) -> Option<TxIdx> {
        if self.is_chain_loc(location) {
            let w = self.chain_live.load(Ordering::Relaxed);
            return (w != usize::MAX).then_some(w);
        }
        self.live_writer.get(&location).map(|e| *e)
    }

    /// Abort / reincarnation: drop version tip + live_writer, wake exact waiters.
    /// Callers must not leave a stale Version tip that parks WaitOnce forever.
    pub(crate) fn clear_writer(
        &self,
        writer: TxIdx,
        locations: &[MemoryLocationHash],
    ) -> Vec<TxIdx> {
        let mut woken = Vec::new();
        if self.chain_loc.load(Ordering::Relaxed) != u64::MAX {
            self.chain_clear(writer);
        }
        for &loc in locations {
            if self.is_chain_loc(loc) {
                woken.extend(self.wake_exact(loc, writer));
                continue;
            }
            if let Some(mut map) = self.tips.get_mut(&loc) {
                map.remove(&writer);
            }
            let _ = self
                .live_writer
                .remove_if(&loc, |_, w| *w == writer);
            woken.extend(self.wake_exact(loc, writer));
        }
        woken
    }

    #[inline]
    pub(crate) fn tip_at(&self, location: MemoryLocationHash, writer: TxIdx) -> Option<SfTip> {
        self.tips
            .get(&location)
            .and_then(|m| m.get(&writer).copied())
    }

    /// True when writer has Released Data tip (or MvMemory will supply Data).
    #[inline]
    pub(crate) fn has_released(&self, location: MemoryLocationHash, writer: TxIdx) -> bool {
        if self.is_chain_loc(location) {
            return self.chain_released(writer);
        }
        matches!(
            self.tip_at(location, writer),
            Some(SfTip::Released { .. })
        )
    }

    /// True when a SpecFence version tip exists (Claimed or Released).
    #[inline]
    pub(crate) fn has_version_or_released(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
    ) -> bool {
        if self.is_chain_loc(location) {
            return self.chain_version_or_released(writer);
        }
        self.tip_at(location, writer).is_some()
    }

    pub(crate) fn register_waiter(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
        consumer: TxIdx,
    ) {
        if consumer <= writer {
            return;
        }
        let mut w = self.waiters.entry((location, writer)).or_default();
        if !w.iter().any(|&c| c == consumer) {
            w.push(consumer);
        }
    }

    pub(crate) fn wake_exact(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
    ) -> Vec<TxIdx> {
        let woken = self
            .waiters
            .remove(&(location, writer))
            .map(|(_, v)| v)
            .unwrap_or_default();
        if !woken.is_empty() {
            self.publish_wake_n
                .fetch_add(woken.len(), Ordering::Relaxed);
        }
        woken
    }

    #[inline]
    pub(crate) fn record_wait_once_consume(&self) {
        self.wait_once_consume_n.fetch_add(1, Ordering::Relaxed);
    }

    /// (a) Detect before read — structure/prior says conflict coming.
    #[inline]
    pub(crate) fn record_detect_before(&self) {
        self.detect_before_n.fetch_add(1, Ordering::Relaxed);
    }

    /// (b) Avoid at read — true publish / done; collision never happens.
    #[inline]
    pub(crate) fn record_avoid_publish(&self) {
        self.avoid_publish_n.fetch_add(1, Ordering::Relaxed);
    }

    /// (c) Resolve after fail — Opt→validate→FullReplay / Rewind.
    #[inline]
    pub(crate) fn record_resolve_after_fail(&self) {
        self.resolve_after_fail_n.fetch_add(1, Ordering::Relaxed);
    }

    /// Four-class timely Avoid (b) — collision never happens for this class.
    #[inline]
    pub(crate) fn record_class_avoid(&self, class: SfConflictClass) {
        match class {
            SfConflictClass::Raw => {
                self.raw_avoid_n.fetch_add(1, Ordering::Relaxed);
            }
            SfConflictClass::War => {
                self.war_avoid_n.fetch_add(1, Ordering::Relaxed);
            }
            SfConflictClass::Waw => {
                self.waw_avoid_n.fetch_add(1, Ordering::Relaxed);
            }
            SfConflictClass::Chain => {
                self.chain_avoid_n.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Four-class late Resolve (c) — FullReplay / Rewind after mistake.
    #[inline]
    pub(crate) fn record_class_late(&self, class: SfConflictClass) {
        match class {
            SfConflictClass::Raw => {
                self.raw_late_n.fetch_add(1, Ordering::Relaxed);
            }
            SfConflictClass::War => {
                self.war_late_n.fetch_add(1, Ordering::Relaxed);
            }
            SfConflictClass::Waw => {
                self.waw_late_n.fetch_add(1, Ordering::Relaxed);
            }
            SfConflictClass::Chain => {
                self.chain_late_n.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// SF path attempted Estimate Block — must stay unused (counter for land proof).
    #[inline]
    pub(crate) fn record_estimate_block_sf(&self) {
        self.estimate_block_sf.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn early_tip_n(&self) -> usize {
        self.early_tip_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn publish_wake_n(&self) -> usize {
        self.publish_wake_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn wait_once_consume_n(&self) -> usize {
        self.wait_once_consume_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn detect_before_n(&self) -> usize {
        self.detect_before_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn avoid_publish_n(&self) -> usize {
        self.avoid_publish_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn resolve_after_fail_n(&self) -> usize {
        self.resolve_after_fail_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn raw_avoid_n(&self) -> usize {
        self.raw_avoid_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn raw_late_n(&self) -> usize {
        self.raw_late_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn war_avoid_n(&self) -> usize {
        self.war_avoid_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn war_late_n(&self) -> usize {
        self.war_late_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn waw_avoid_n(&self) -> usize {
        self.waw_avoid_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn waw_late_n(&self) -> usize {
        self.waw_late_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn chain_avoid_n(&self) -> usize {
        self.chain_avoid_n.load(Ordering::Relaxed)
    }
    #[inline]
    pub(crate) fn chain_late_n(&self) -> usize {
        self.chain_late_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn estimate_block_sf(&self) -> usize {
        self.estimate_block_sf.load(Ordering::Relaxed)
    }
}

/// Four conflict classes SfMvMemory must serve (concurrent Detect|Avoid|Resolve).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SfConflictClass {
    /// Reader must see correct published write.
    Raw,
    /// Write-after-read: invalidate / revalidate higher readers — not schedule-only.
    War,
    /// Publish order / WaitOnce / OrderedTip.
    Waw,
    /// Long dependency chain (sticky ≥32 / nearest-pred).
    Chain,
}

/// Classify a WaitOnce / crit consult edge for four-class audit.
/// Chain wins on sticky ≥32 crit ℓ; early-WAW template (k>0) → Waw; else Raw.
#[inline]
pub(crate) fn classify_wait_conflict(
    is_crit: bool,
    crit_len: usize,
    wait_once_k: u32,
) -> SfConflictClass {
    if is_crit && crit_len >= 32 {
        SfConflictClass::Chain
    } else if wait_once_k > 0 || is_crit {
        SfConflictClass::Waw
    } else {
        SfConflictClass::Raw
    }
}

/// SpecFence version plane. Wraps [`MvMemory`] + [`SfTipTable`].
pub(crate) struct SfMvMemory<'a> {
    inner: &'a MvMemory,
    tips: &'a SfTipTable,
}

impl<'a> SfMvMemory<'a> {
    #[inline]
    pub(crate) fn new(inner: &'a MvMemory, tips: &'a SfTipTable) -> Self {
        Self { inner, tips }
    }

    #[inline]
    pub(crate) fn inner(&self) -> &'a MvMemory {
        self.inner
    }

    #[inline]
    pub(crate) fn tips(&self) -> &'a SfTipTable {
        self.tips
    }

    /// Read `location` for `tx` under `vis`.
    ///
    /// - [`VisibilityPolicy::Opt`]: race tip (Data or Estimate). Independent set.
    /// - [`VisibilityPolicy::WaitReleased`]: last live Data; never the Estimate tip.
    /// - [`VisibilityPolicy::OrderedTip`]: same as WaitReleased — the installed
    ///   ordered producer, not a concurrent unfinished writer.
    pub(crate) fn read(
        &self,
        location: MemoryLocationHash,
        vis: VisibilityPolicy,
        tx: TxIdx,
    ) -> SfRead {
        if tx == 0 {
            return SfRead::Storage;
        }
        match vis {
            VisibilityPolicy::Opt => self.read_opt(location, tx),
            VisibilityPolicy::WaitReleased | VisibilityPolicy::OrderedTip => {
                self.read_released_or_tip(location, tx)
            }
        }
    }

    /// Installed (writer, incarnation) for edged reads. `None` ⇒ storage.
    #[inline]
    pub(crate) fn read_tip(
        &self,
        location: MemoryLocationHash,
        vis: VisibilityPolicy,
        tx: TxIdx,
    ) -> Option<(TxIdx, TxIncarnation)> {
        match self.read(location, vis, tx) {
            SfRead::Data {
                writer,
                incarnation,
                ..
            } => Some((writer, incarnation)),
            _ => None,
        }
    }

    /// True publish for `writer` on `location` (Released tip, ChainSpine, or MvMemory Data).
    pub(crate) fn true_publish_ready(
        &self,
        location: MemoryLocationHash,
        writer: TxIdx,
    ) -> bool {
        if self.tips.has_released(location, writer) {
            return true;
        }
        matches!(
            self.inner.entry_kind_at(location, writer),
            "data"
        )
    }

    fn read_opt(&self, location: MemoryLocationHash, tx: TxIdx) -> SfRead {
        let Some(written) = self.inner.data.get(&location) else {
            return SfRead::Storage;
        };
        match written.range(..tx).next_back() {
            Some((w, crate::MemoryEntry::Estimate)) => SfRead::Estimate { writer: *w },
            Some((w, crate::MemoryEntry::Data(inc, value))) => {
                if self.inner.is_aborted_incarnation(*w, *inc) {
                    SfRead::Estimate { writer: *w }
                } else {
                    SfRead::Data {
                        writer: *w,
                        incarnation: *inc,
                        value: value.clone(),
                    }
                }
            }
            None => SfRead::Storage,
        }
    }

    fn read_released_or_tip(&self, location: MemoryLocationHash, tx: TxIdx) -> SfRead {
        if let Some((w, inc)) = self.inner.last_data_before(location, tx)
            && let Some(value) = self.inner.published_data_value(w, location)
        {
            return SfRead::Data {
                writer: w,
                incarnation: inc,
                value,
            };
        }
        SfRead::Storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mv_memory::MvMemory;
    use crate::{MemoryEntry, TxVersion};

    fn empty_mv(n: usize) -> MvMemory {
        MvMemory::new(n, std::iter::empty(), std::iter::empty())
    }

    #[test]
    fn edged_read_skips_estimate_tip() {
        let mv = empty_mv(3);
        let tips = SfTipTable::new();
        mv.data
            .entry(7)
            .or_default()
            .insert(0, MemoryEntry::Estimate);
        let sf = SfMvMemory::new(&mv, &tips);
        match sf.read(7, VisibilityPolicy::WaitReleased, 2) {
            SfRead::Storage => {}
            other => panic!("WaitReleased must not return Estimate tip: {other:?}"),
        }
        match sf.read(7, VisibilityPolicy::OrderedTip, 2) {
            SfRead::Storage => {}
            other => panic!("OrderedTip must not return Estimate tip: {other:?}"),
        }
        match sf.read(7, VisibilityPolicy::Opt, 2) {
            SfRead::Estimate { writer } => assert_eq!(writer, 0),
            other => panic!("Opt may see Estimate: {other:?}"),
        }
    }

    #[test]
    fn ordered_tip_reads_installed_data() {
        let mv = empty_mv(3);
        let tips = SfTipTable::new();
        let ver = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        let mut ws = crate::WriteSet::new();
        ws.push((
            9,
            crate::MemoryValue::Storage(alloy_primitives::U256::from(42u64)),
        ));
        mv.record(&ver, crate::ReadSet::default(), ws);
        tips.publish_data(9, 0, 0);
        let sf = SfMvMemory::new(&mv, &tips);
        match sf.read(9, VisibilityPolicy::OrderedTip, 2) {
            SfRead::Data {
                writer,
                incarnation,
                ..
            } => {
                assert_eq!(writer, 0);
                assert_eq!(incarnation, 0);
            }
            other => panic!("expected Data, got {other:?}"),
        }
        assert!(sf.true_publish_ready(9, 0));
    }

    #[test]
    fn version_tip_is_not_estimate_and_wakes_exact() {
        let tips = SfTipTable::new();
        tips.install_version_tip(11, 1, 0);
        assert!(matches!(
            tips.tip_at(11, 1),
            Some(SfTip::Version { incarnation: 0 })
        ));
        assert_eq!(tips.live_writer(11), Some(1));
        tips.register_waiter(11, 1, 3);
        tips.register_waiter(11, 1, 4);
        let woken = tips.publish_data(11, 1, 0);
        assert_eq!(woken, vec![3, 4]);
        assert!(tips.has_released(11, 1));
        assert_eq!(tips.live_writer(11), None);
        assert_eq!(tips.estimate_block_sf(), 0);
        assert_eq!(tips.early_tip_n(), 1);
        assert_eq!(tips.publish_wake_n(), 2);
    }

    #[test]
    fn chain_spine_claim_release_without_dashmap_tips() {
        let tips = SfTipTable::new();
        let writers = vec![10usize, 20, 30, 40];
        tips.bind_chain_spine(0xabc, &writers);
        assert!(tips.is_chain_loc(0xabc));
        tips.chain_claim(20, 0);
        assert_eq!(tips.early_tip_n(), 1);
        assert_eq!(tips.live_writer(0xabc), Some(20));
        assert!(tips.has_version_or_released(0xabc, 20));
        assert!(!tips.has_released(0xabc, 20));
        // Reclaim same writer must not bump early_tip again from EMPTY.
        tips.chain_claim(20, 1);
        assert_eq!(tips.early_tip_n(), 1);
        tips.publish_data(0xabc, 20, 0);
        assert!(tips.has_released(0xabc, 20));
        assert!(tips.live_writer(0xabc).is_none());
        // DashMap tips unused for chain loc.
        assert!(tips.tip_at(0xabc, 20).is_none());
    }

    #[test]
    fn install_version_tip_skips_same_incarnation() {
        let tips = SfTipTable::new();
        tips.install_version_tip(7, 1, 0);
        assert_eq!(tips.early_tip_n(), 1);
        tips.install_version_tip(7, 1, 0);
        assert_eq!(tips.early_tip_n(), 1, "same Version inc must not re-census");
    }
}
