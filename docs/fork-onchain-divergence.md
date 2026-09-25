# Fork on-chain divergence

Unmodified upstream `risechain/pevm` `e94b0e3` matches chain receipts and gas on blocks 3356896 and 15274915. This fork disagrees in sequential mode because `PevmEthereum::build_evm` replaces `SLOAD` with a wrapper whose static gas is 0. The interpreter charges that static gas before the handler, and stock `sload` does not charge it again.

## Upstream run

**Same assertion as `mainnet_blocks_from_disk`.** Worktree at `e94b0e3df9c0c1b983ed3e4d78b3aefb0b5b7cc0`, disk snapshots shared with this repo. One `test_execute_alloy(..., must_match_block_header = true)` per block: sequential result equals parallel, then receipt root, logs bloom, and cumulative gas against the header.

| Block | Spec | Result |
| --- | --- | --- |
| 3356896 | Spurious Dragon | passed |
| 15274915 | London | passed |

Release build, `lto` off. The test finished in 0.71s, exit 0. Both sides use revm 38.0.0 from crates.io. This fork's `Cargo.lock` at `7699508` pins the same revm, with no `[patch]`.

## The hunk

**Introduced in `3a9a9451` (`feat(specfence): ABC Iter19 — Bind-snap at certified-prefix end`).** The install passed static gas 0:

```rust
instructions.insert_instruction(
    OP_SLOAD,
    revm::interpreter::Instruction::new(sload_bind_snap_eth::<H>, 0),
);
```

Rename `cb38085` kept the 0. On this branch (`7699508`) the function is `install_handler_ordered_admit_snap_capture` in `crates/pevm/src/specfence/boundary.rs` (lines 2227–2236), and the callee is `sload_ordered_admit_snap_eth`. `PevmEthereum::build_evm` (`crates/pevm/src/chain/ethereum.rs`, lines 143–145) installs it when `handler_ordered_admit_snap_install_wanted()` is true.

That predicate is true whenever OrderedAdmit-snap mode is not `Off` (`boundary.rs` `ordered_admit_snap_capture_wanted`, lines 2010–2012). Unset `SPECFENCE_BIND_SNAP` is `ResumePath` (`ordered_admit_snap_mode`, lines 1978–1980), so every mainnet EVM gets the wrapper, including sequential execution.

`execute_revm_sequential` (`pevm.rs`) builds that EVM and calls `evm.transact`. Sequential and parallel therefore share the undercharge, and `seq == par` still fails the header.

## Mechanism

**`insert_instruction` replaces the whole `Instruction`, including `static_gas`.** revm-interpreter charges `instruction.static_gas()` before calling the handler (`interpreter.rs`). The wrapper then calls stock `host::sload`.

Pre-Berlin `host::sload` adds no dynamic gas. The spec table is the whole cost (`revm-interpreter` `instructions.rs`): Frontier 50, Tangerine and Spurious Dragon 200, Istanbul 800. Berlin and later set static gas to `WARM_STORAGE_READ_COST` (100) and add only the cold surcharge inside `host::sload`.

Block 3356896 is Spurious Dragon. The fork's reported header gap is `4033966 - 4014166 = 19800 = 99 × 200`: each executed `SLOAD` paid 0 instead of 200. Bloom can still match, because the value read is the stock one.

Block 15274915 is London. Each `SLOAD` misses the warm 100. Cumulative gas is part of the receipt, so the receipt root moves with the gas. A contract that observes `GAS` can also take a different path; the root mismatch does not require that.

The SSTORE wrapper is also installed with static gas 0, and stock `SSTORE` static gas is already 0. `handler_sstore_protocol_install_wanted` stays false unless research inspect, absolute jump, or `SPECFENCE_HANDLER_CAPTURE` is on. Default sequential execution does not install it.

## Where the other candidates run

**They do not meter sequential `SLOAD`.**

- **NESTED_BIND** resumes a saved frame in the parallel runner. `execute_revm_sequential` never enters it.
- **Access-event host changes** sit on the parallel VmDb path. Sequential `transact` uses `CacheDB`.
- **Host settlement** is the parallel multi-version read. The sequential host is stock revm.

## Rule for a clean port

Copy the table's current `static_gas()` into `Instruction::new` when replacing an opcode. The spec-adjusted table is already built by `build_mainnet_with_inspector` before the wrapper is installed. A literal `0` is the right static gas only for an opcode whose stock static gas is already 0.
