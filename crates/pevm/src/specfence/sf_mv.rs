//! SfMvMemory — SpecFence visibility-aware version plane (SF-PS T2).
//!
//! `read(ℓ, vis)` is the SpecFence read API. Occupied OCC keeps walking
//! `MvMemory` without a policy. Edged SpecFence txs must not enter that
//! default OCC tip walk.

use crate::mv_memory::MvMemory;
use crate::{MemoryLocationHash, MemoryValue, TxIdx, TxIncarnation};

use super::VisibilityPolicy;

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

/// SpecFence version plane. Wraps [`MvMemory`]; does not replace the OCC store.
pub(crate) struct SfMvMemory<'a> {
    inner: &'a MvMemory,
}

impl<'a> SfMvMemory<'a> {
    #[inline]
    pub(crate) fn new(inner: &'a MvMemory) -> Self {
        Self { inner }
    }

    #[inline]
    pub(crate) fn inner(&self) -> &'a MvMemory {
        self.inner
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
        mv.data
            .entry(7)
            .or_default()
            .insert(0, MemoryEntry::Estimate);
        let sf = SfMvMemory::new(&mv);
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
        let sf = SfMvMemory::new(&mv);
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
    }
}
