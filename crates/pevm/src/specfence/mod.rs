//! `SpecFence`, an in-block ordered-writer engine.
//!
//! Built beside upstream Block-STM. `Pevm::execute` and `execute_revm_parallel`
//! do not call this module. With the `specfence` feature off, this module is
//! not compiled.

mod deque;
mod engine;
mod live_chain;
mod mv;
mod rt;
mod trace;
mod vm;

pub use engine::{SfClassKey, SfOptions, last_trace, run_sf_block};
pub use trace::{SfAttempt, SfTrace};

/// Revm block environment for an Alloy header. Same mapping upstream uses.
pub fn block_env(
    header: &alloy_rpc_types_eth::Header,
    spec_id: impl Into<revm::primitives::hardfork::SpecId>,
) -> revm::context::BlockEnv {
    crate::compat::get_block_env(header, spec_id)
}
