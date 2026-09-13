//! Temporary instrumentation: decision-field contingencies for π grain selection.
//!
//! Research-only. Aggregates feature × verb counts at `choose_edge_action` time
//! so lab can score fields as decision-useful / redundant / harmful / observe-only.
//! Not a control plane.

#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// Verb class after `choose_edge_action` (+ demotion proxies via reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecisionVerb {
    Bind = 0,
    WaitFor = 1,
    Unfenced = 2,
}

const VERB_N: usize = 3;

/// Bucketed feature × verb contingency (one block).
#[derive(Debug, Default)]
pub(crate) struct DecisionFieldAgg {
    total: AtomicU64,
    /// verb histogram
    verb: [AtomicU64; VERB_N],
    /// Binary features: feat_idx → [false×3verbs | true×3verbs]
    /// Order documented in `DecisionFieldSnap::feature_names`.
    bin: [[AtomicU64; VERB_N * 2]; 16],
    /// k buckets 0..4 × verb
    k_bucket: [[AtomicU64; VERB_N]; 5],
    /// depth buckets 0..3 × verb
    depth_bucket: [[AtomicU64; VERB_N]; 4],
    /// incarnation buckets 0 / 1 / 2+ × verb
    inc_bucket: [[AtomicU64; VERB_N]; 3],
    /// writer status: none/ready/exec/validated/other × verb
    writer_status: [[AtomicU64; VERB_N]; 5],
    /// edge kind proxy: wr only today (slot reserved)
    edge_kind_wr: [AtomicU64; VERB_N],
    /// Quality proxies
    missed_avoid: AtomicU64,   // Unfenced while avoid/essential/force
    false_fence_wait: AtomicU64, // Wait while published already (shouldn't)
    wait_no_writer: AtomicU64,
    bind_no_publish: AtomicU64,
    unfenced_essential: AtomicU64,
}

/// Compact feature vector at decision time (lab).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecisionFeat {
    pub verb: DecisionVerb,
    pub access_k: u32,
    pub depth: u8,
    pub incarnation: usize,
    pub is_program: bool,
    pub writer_published: bool,
    pub writer_validated: bool,
    pub writer_executing: bool,
    pub writer_ready: bool,
    pub writer_present: bool,
    pub avoid_broadcast: bool,
    pub canary_ok: bool,
    pub independence_certified: bool,
    pub essential_antidep: bool,
    pub force_prefix: bool,
    pub clique_gated: bool,
    pub in_hot_set: bool,
    pub prior_warm: bool, // prior_ws || sticky
    pub mode_read: bool,  // maybe_wait is always read today
}

impl DecisionFieldAgg {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn bump_bin(slot: &[AtomicU64; VERB_N * 2], true_bit: bool, verb: DecisionVerb) {
        let base = if true_bit { VERB_N } else { 0 };
        slot[base + verb as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record(&self, f: DecisionFeat) {
        let v = f.verb as usize;
        self.total.fetch_add(1, Ordering::Relaxed);
        self.verb[v].fetch_add(1, Ordering::Relaxed);
        self.edge_kind_wr[v].fetch_add(1, Ordering::Relaxed);

        // Binary features (stable index order)
        Self::bump_bin(&self.bin[0], f.is_program, f.verb);
        Self::bump_bin(&self.bin[1], f.writer_published, f.verb);
        Self::bump_bin(&self.bin[2], f.writer_validated, f.verb);
        Self::bump_bin(&self.bin[3], f.writer_executing, f.verb);
        Self::bump_bin(&self.bin[4], f.writer_ready, f.verb);
        Self::bump_bin(&self.bin[5], f.writer_present, f.verb);
        Self::bump_bin(&self.bin[6], f.avoid_broadcast, f.verb);
        Self::bump_bin(&self.bin[7], f.canary_ok, f.verb);
        Self::bump_bin(&self.bin[8], f.independence_certified, f.verb);
        Self::bump_bin(&self.bin[9], f.essential_antidep, f.verb);
        Self::bump_bin(&self.bin[10], f.force_prefix, f.verb);
        Self::bump_bin(&self.bin[11], f.clique_gated, f.verb);
        Self::bump_bin(&self.bin[12], f.in_hot_set, f.verb);
        Self::bump_bin(&self.bin[13], f.prior_warm, f.verb);
        Self::bump_bin(&self.bin[14], f.mode_read, f.verb);
        // bin[15] reserved (storage_vs_account ≈ is_program today)

        let kb = match f.access_k {
            0 => 0,
            1..=3 => 1,
            4..=7 => 2,
            8..=15 => 3,
            _ => 4,
        };
        self.k_bucket[kb][v].fetch_add(1, Ordering::Relaxed);

        let db = match f.depth {
            0 => 0,
            1..=3 => 1,
            4..=7 => 2,
            _ => 3,
        };
        self.depth_bucket[db][v].fetch_add(1, Ordering::Relaxed);

        let ib = match f.incarnation {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        self.inc_bucket[ib][v].fetch_add(1, Ordering::Relaxed);

        let ws = if !f.writer_present {
            0
        } else if f.writer_validated {
            3
        } else if f.writer_executing {
            2
        } else if f.writer_ready {
            1
        } else {
            4
        };
        self.writer_status[ws][v].fetch_add(1, Ordering::Relaxed);

        // Quality proxies (correlational)
        let should_fence = f.essential_antidep || f.avoid_broadcast || f.force_prefix;
        if matches!(f.verb, DecisionVerb::Unfenced) && should_fence {
            self.missed_avoid.fetch_add(1, Ordering::Relaxed);
            self.unfenced_essential.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(f.verb, DecisionVerb::WaitFor) && f.writer_published {
            self.false_fence_wait.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(f.verb, DecisionVerb::WaitFor) && !f.writer_present {
            self.wait_no_writer.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(f.verb, DecisionVerb::Bind) && !f.writer_published {
            self.bind_no_publish.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn snapshot(&self) -> DecisionFieldSnap {
        let load3 = |a: &[AtomicU64; VERB_N]| -> [u64; 3] {
            [
                a[0].load(Ordering::Relaxed),
                a[1].load(Ordering::Relaxed),
                a[2].load(Ordering::Relaxed),
            ]
        };
        let mut bin_out = BTreeMap::new();
        for (i, name) in FEATURE_NAMES.iter().enumerate() {
            let slot = &self.bin[i];
            bin_out.insert(
                (*name).to_string(),
                BTreeMap::from([
                    (
                        "false".to_string(),
                        VerbHist {
                            bind: slot[0].load(Ordering::Relaxed),
                            wait_for: slot[1].load(Ordering::Relaxed),
                            unfenced: slot[2].load(Ordering::Relaxed),
                        },
                    ),
                    (
                        "true".to_string(),
                        VerbHist {
                            bind: slot[3].load(Ordering::Relaxed),
                            wait_for: slot[4].load(Ordering::Relaxed),
                            unfenced: slot[5].load(Ordering::Relaxed),
                        },
                    ),
                ]),
            );
        }
        let k_names = ["k0", "k1_3", "k4_7", "k8_15", "k16p"];
        let mut k_out = BTreeMap::new();
        for (i, n) in k_names.iter().enumerate() {
            let c = load3(&self.k_bucket[i]);
            k_out.insert(
                (*n).to_string(),
                VerbHist {
                    bind: c[0],
                    wait_for: c[1],
                    unfenced: c[2],
                },
            );
        }
        let d_names = ["d0", "d1_3", "d4_7", "d8p"];
        let mut d_out = BTreeMap::new();
        for (i, n) in d_names.iter().enumerate() {
            let c = load3(&self.depth_bucket[i]);
            d_out.insert(
                (*n).to_string(),
                VerbHist {
                    bind: c[0],
                    wait_for: c[1],
                    unfenced: c[2],
                },
            );
        }
        let i_names = ["inc0", "inc1", "inc2p"];
        let mut i_out = BTreeMap::new();
        for (i, n) in i_names.iter().enumerate() {
            let c = load3(&self.inc_bucket[i]);
            i_out.insert(
                (*n).to_string(),
                VerbHist {
                    bind: c[0],
                    wait_for: c[1],
                    unfenced: c[2],
                },
            );
        }
        let w_names = ["none", "ready", "executing", "validated", "other"];
        let mut w_out = BTreeMap::new();
        for (i, n) in w_names.iter().enumerate() {
            let c = load3(&self.writer_status[i]);
            w_out.insert(
                (*n).to_string(),
                VerbHist {
                    bind: c[0],
                    wait_for: c[1],
                    unfenced: c[2],
                },
            );
        }
        let vc = load3(&self.verb);
        DecisionFieldSnap {
            n_decisions: self.total.load(Ordering::Relaxed),
            verb: VerbHist {
                bind: vc[0],
                wait_for: vc[1],
                unfenced: vc[2],
            },
            binary: bin_out,
            k_bucket: k_out,
            depth_bucket: d_out,
            inc_bucket: i_out,
            writer_status: w_out,
            edge_kind_wr: {
                let c = load3(&self.edge_kind_wr);
                VerbHist {
                    bind: c[0],
                    wait_for: c[1],
                    unfenced: c[2],
                }
            },
            quality: QualityProxies {
                missed_avoid: self.missed_avoid.load(Ordering::Relaxed),
                false_fence_wait: self.false_fence_wait.load(Ordering::Relaxed),
                wait_no_writer: self.wait_no_writer.load(Ordering::Relaxed),
                bind_no_publish: self.bind_no_publish.load(Ordering::Relaxed),
                unfenced_essential: self.unfenced_essential.load(Ordering::Relaxed),
            },
        }
    }
}

const FEATURE_NAMES: [&str; 16] = [
    "is_program",
    "writer_published",
    "writer_validated",
    "writer_executing",
    "writer_ready",
    "writer_present",
    "avoid_broadcast",
    "canary_ok",
    "independence_certified",
    "essential_antidep",
    "force_prefix",
    "clique_gated",
    "in_hot_set_H",
    "prior_warm",
    "mode_read",
    "reserved",
];

#[derive(Debug, Clone, Serialize, Default)]
pub struct VerbHist {
    pub bind: u64,
    pub wait_for: u64,
    pub unfenced: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QualityProxies {
    pub missed_avoid: u64,
    pub false_fence_wait: u64,
    pub wait_no_writer: u64,
    pub bind_no_publish: u64,
    pub unfenced_essential: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DecisionFieldSnap {
    pub n_decisions: u64,
    pub verb: VerbHist,
    pub binary: BTreeMap<String, BTreeMap<String, VerbHist>>,
    pub k_bucket: BTreeMap<String, VerbHist>,
    pub depth_bucket: BTreeMap<String, VerbHist>,
    pub inc_bucket: BTreeMap<String, VerbHist>,
    pub writer_status: BTreeMap<String, VerbHist>,
    pub edge_kind_wr: VerbHist,
    pub quality: QualityProxies,
}

impl DecisionFieldSnap {
    pub fn merge_from(&mut self, other: &Self) {
        self.n_decisions += other.n_decisions;
        self.verb.bind += other.verb.bind;
        self.verb.wait_for += other.verb.wait_for;
        self.verb.unfenced += other.verb.unfenced;
        self.edge_kind_wr.bind += other.edge_kind_wr.bind;
        self.edge_kind_wr.wait_for += other.edge_kind_wr.wait_for;
        self.edge_kind_wr.unfenced += other.edge_kind_wr.unfenced;
        self.quality.missed_avoid += other.quality.missed_avoid;
        self.quality.false_fence_wait += other.quality.false_fence_wait;
        self.quality.wait_no_writer += other.quality.wait_no_writer;
        self.quality.bind_no_publish += other.quality.bind_no_publish;
        self.quality.unfenced_essential += other.quality.unfenced_essential;
        for (k, v) in &other.binary {
            let e = self.binary.entry(k.clone()).or_default();
            for (bk, vh) in v {
                let t = e.entry(bk.clone()).or_default();
                t.bind += vh.bind;
                t.wait_for += vh.wait_for;
                t.unfenced += vh.unfenced;
            }
        }
        for (map, src) in [
            (&mut self.k_bucket, &other.k_bucket),
            (&mut self.depth_bucket, &other.depth_bucket),
            (&mut self.inc_bucket, &other.inc_bucket),
            (&mut self.writer_status, &other.writer_status),
        ] {
            for (k, vh) in src {
                let t = map.entry(k.clone()).or_default();
                t.bind += vh.bind;
                t.wait_for += vh.wait_for;
                t.unfenced += vh.unfenced;
            }
        }
    }
}
