//! `SpecFence`, an in-block ordered-writer engine.
//!
//! Built beside upstream Block-STM. `Pevm::execute` and `execute_revm_parallel`
//! do not call this module. With the `specfence` feature off, this module is
//! not compiled.
//!
//! Opcode or host wrappers belong on this engine only. `PevmChain::build_evm`
//! stays the stock builder, so sequential execution and upstream OCC never see
//! them. A replacement must keep the table's `static_gas()` for that spec.
//! The interpreter charges that static gas before the handler, and stock
//! `SLOAD` does not charge it again.

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
