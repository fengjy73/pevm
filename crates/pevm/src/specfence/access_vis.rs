//! EdgeVisibility — `unfinished` counts **only !done** writers (v6 S2).
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` §3.1.
//!
//! FORBIDDEN: pushing `last_writer_before` into `unfinished` when `is_done`.
//! `Data ∧ unfinished=0 → OrderedAdmit` is reachable only with this filter.

use crate::TxIdx;

/// Compose \(e_{\mathrm{vis}}\) unfinished + writer identity.
///
/// `sketch` is already `!is_done` filtered. `mv_writer` / `residual` / `force`
/// are raw MV tips and must be dropped when done.
#[inline]
pub(crate) fn compose_unfinished(
    mut sketch: Vec<TxIdx>,
    mv_writer: Option<TxIdx>,
    residual: Option<TxIdx>,
    force: Option<TxIdx>,
    reader: TxIdx,
    is_done: impl Fn(TxIdx) -> bool,
) -> (Option<TxIdx>, usize) {
    sketch.retain(|&w| w < reader && !is_done(w));
    let tip = mv_writer
        .or(residual)
        .or(force)
        .filter(|&w| w < reader)
        .or_else(|| sketch.iter().copied().min());
    if let Some(w) = tip
        && !is_done(w)
        && !sketch.contains(&w)
    {
        sketch.push(w);
    }
    (tip, sketch.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_mv_tip_is_not_unfinished() {
        let (writer, n) = compose_unfinished(Vec::new(), Some(1), None, None, 4, |w| w == 1);
        assert_eq!(writer, Some(1));
        assert_eq!(n, 0, "S2: done last_writer must not starve OrderedAdmit");
    }

    #[test]
    fn live_mv_tip_counts() {
        let (writer, n) = compose_unfinished(Vec::new(), Some(2), None, None, 4, |_| false);
        assert_eq!(writer, Some(2));
        assert_eq!(n, 1);
    }

    #[test]
    fn sketch_unfinished_kept() {
        let (writer, n) = compose_unfinished(vec![0, 2], Some(2), None, None, 5, |w| w == 0);
        assert_eq!(writer, Some(2));
        assert_eq!(n, 1);
    }

    #[test]
    fn force_writer_dropped_when_done() {
        let (_, n) = compose_unfinished(Vec::new(), None, None, Some(3), 8, |_| true);
        assert_eq!(n, 0);
    }
}
