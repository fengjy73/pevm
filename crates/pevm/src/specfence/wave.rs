//! Wave park / ready deque — **product** scheduling surface (file-SRP).
//!
//! SoftWait Soft + SuffixRepair research stay in [`super::rem`] (quarantined;
//! Soft=0). This module is the only hot-path WavePark owner name.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.4-file-srp.md`.

pub(crate) use super::rem::{
    ParkKind, ParkResumeIntent, ParkResumeKind, ParkedWait, PendingPark, WaveParkTable,
};
