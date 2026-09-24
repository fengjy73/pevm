//! Shared no-beneficiary Handler for pevm execute paths.

use std::cell::Cell;

use revm::{
    Database, Inspector,
    context::{
        ContextTr, JournalTr,
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction},
    },
    handler::{EthFrame, EvmTr, EvmTrError, FrameResult, FrameTr, Handler, ItemOrResult},
    inspector::{InspectorEvmTr, InspectorHandler, JournalExt},
    interpreter::{
        interpreter::EthInterpreter,
        interpreter_types::{Jumps, LoopControl},
    },
    state::EvmState,
};

use crate::chain::{PevmChain, PevmEthereum};
use crate::specfence::{
    nested_ordered_admit_stash_armed, pending_resume_armed, try_apply_pending_pc_resume,
    try_consume_nested_ordered_admit_resume,
};

/// MainnetHandler that skips beneficiary reward (pevm applies via MvMemory).
pub(crate) struct NoBeneficiaryHandler<EVM, ERROR> {
    _phantom: core::marker::PhantomData<(EVM, ERROR)>,
}

impl<EVM, ERROR> Default for NoBeneficiaryHandler<EVM, ERROR> {
    fn default() -> Self {
        Self {
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<EVM, ERROR> Handler for NoBeneficiaryHandler<EVM, ERROR>
where
    EVM: EvmTr<
            Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt>,
            Frame = EthFrame<EthInterpreter>,
        >,
    ERROR: EvmTrError<EVM>,
{
    type Evm = EVM;
    type Error = ERROR;
    type HaltReason = HaltReason;

    fn reward_beneficiary(
        &self,
        _: &mut Self::Evm,
        _: &mut FrameResult,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// YieldWait does not discard the journal or the frame stack.
    ///
    /// The successful WaitTrueVersion path never returns this error: it waits
    /// inside the host call and continues the same opcode. This override is the
    /// deadlock release: one resume of the halted opcode (frame kept), then
    /// `catch_error` only if the tip is still missing.
    fn run(
        &mut self,
        evm: &mut Self::Evm,
    ) -> Result<ExecutionResult<Self::HaltReason>, Self::Error> {
        clear_frame_depth();
        let mut init_and_floor_gas = match self.validate(evm) {
            Ok(gas) => gas,
            Err(e) => return self.catch_error(evm, e),
        };
        let eip7702_refund = match self.pre_execution(evm, &mut init_and_floor_gas) {
            Ok(refund) => refund,
            Err(e) => return self.catch_error(evm, e),
        };
        let mut exec_result = match self.execution(evm, &init_and_floor_gas) {
            Ok(result) => result,
            Err(e) if take_yield_pending() => match self.resume_yield(evm, e) {
                Ok(result) => result,
                Err(e) => return Err(e),
            },
            Err(e) => return self.catch_error(evm, e),
        };
        let result_gas = match self.post_execution(
            evm,
            &mut exec_result,
            init_and_floor_gas,
            eip7702_refund as i64,
        ) {
            Ok(gas) => gas,
            Err(e) => return self.catch_error(evm, e),
        };
        match self.execution_result(evm, exec_result, result_gas) {
            Ok(out) => Ok(out),
            Err(e) => self.catch_error(evm, e),
        }
    }

    /// Iter8: apply armed PENDING_RESUME on Handler::run (no inspect_run).
    /// Stock revm only applies via Inspector::initialize_interp; we hook after
    /// first frame_init so memory-lite absolute jump works hang-free on Lean.
    #[inline]
    fn run_exec_loop(
        &mut self,
        evm: &mut Self::Evm,
        first_frame_input: <<Self::Evm as EvmTr>::Frame as FrameTr>::FrameInit,
    ) -> Result<FrameResult, Self::Error> {
        let res = evm.frame_init(first_frame_input)?;

        if let ItemOrResult::Result(frame_result) = res {
            return Ok(frame_result);
        }

        // Apply PC/stack/memory/write_replays before first frame_run.
        // frame_stack and ctx are distinct Evm fields — split via raw pointers.
        if pending_resume_armed() {
            let evm_ptr = evm as *mut Self::Evm;
            // SAFETY: ctx and frame_stack are disjoint fields; no aliasing with
            // other borrows for the duration of try_apply_pending_pc_resume.
            unsafe {
                let frame = (*evm_ptr).frame_stack().get();
                let depth = frame.depth.min(u16::MAX as usize) as u16;
                let interp = &mut frame.interpreter;
                let ctx = (*evm_ptr).ctx();
                try_apply_pending_pc_resume(interp, ctx, depth);
            }
        }

        loop {
            note_frame_depth(evm);
            let call_or_result = evm.frame_run()?;

            let result = match call_or_result {
                ItemOrResult::Item(init) => match evm.frame_init(init)? {
                    ItemOrResult::Item(_) => {
                        // Iter29: hang-free nested OrderedAdmit consume after natural CALL
                        // enters a new frame — stash is consulted only here (PENDING
                        // cleared on mismatch; ≠ Iter28e frame_init-defer).
                        if nested_ordered_admit_stash_armed() {
                            let evm_ptr = evm as *mut Self::Evm;
                            unsafe {
                                let frame = (*evm_ptr).frame_stack().get();
                                let depth = frame.depth.min(u16::MAX as usize) as u16;
                                let interp = &mut frame.interpreter;
                                let ctx = (*evm_ptr).ctx();
                                try_consume_nested_ordered_admit_resume(interp, ctx, depth);
                            }
                        }
                        continue;
                    }
                    ItemOrResult::Result(result) => result,
                },
                ItemOrResult::Result(result) => result,
            };

            if let Some(result) = evm.frame_return_result(result)? {
                return Ok(result);
            }
        }
    }
}

impl<EVM, ERROR> NoBeneficiaryHandler<EVM, ERROR>
where
    EVM: EvmTr<
            Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt>,
            Frame = EthFrame<EthInterpreter>,
        >,
    ERROR: EvmTrError<EVM>,
{
    /// Continue the live frame after YieldWait. Does not call `discard_tx`.
    fn resume_yield(&mut self, evm: &mut EVM, first_err: ERROR) -> Result<FrameResult, ERROR> {
        // One in-frame retry. A second YieldWait releases the core.
        if !prep_yield_resume(evm) {
            return self.discard_frame(evm, first_err);
        }
        match self.continue_frame(evm) {
            Ok(mut frame_result) => match self.last_frame_result(evm, &mut frame_result) {
                Ok(()) => Ok(frame_result),
                Err(e) => self.discard_frame(evm, e),
            },
            Err(e) => self.discard_frame(evm, e),
        }
    }

    fn discard_frame(&self, evm: &mut EVM, error: ERROR) -> Result<FrameResult, ERROR> {
        let _ = take_yield_pending();
        match self.catch_error(evm, error) {
            Err(err) => Err(err),
            Ok(_) => unreachable!("catch_error propagates the database error"),
        }
    }

    /// `run_exec_loop` without `frame_init` — the halted frame is still on the stack.
    fn continue_frame(&mut self, evm: &mut EVM) -> Result<FrameResult, ERROR> {
        loop {
            note_frame_depth(evm);
            let call_or_result = evm.frame_run()?;
            let result = match call_or_result {
                ItemOrResult::Item(init) => match evm.frame_init(init)? {
                    ItemOrResult::Item(_) => continue,
                    ItemOrResult::Result(result) => result,
                },
                ItemOrResult::Result(result) => result,
            };
            if let Some(result) = evm.frame_return_result(result)? {
                return Ok(result);
            }
        }
    }
}

impl<EVM, ERROR> InspectorHandler for NoBeneficiaryHandler<EVM, ERROR>
where
    EVM: InspectorEvmTr<
            Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt>,
            Frame = EthFrame<EthInterpreter>,
            Inspector: Inspector<<EVM as EvmTr>::Context, EthInterpreter>,
        >,
    ERROR: EvmTrError<EVM>,
{
    type IT = EthInterpreter;
}

pub(crate) type EthDbError<DB> = EVMError<<DB as Database>::Error, InvalidTransaction>;

thread_local! {
    static FRAME_DEPTH: Cell<u8> = const { Cell::new(0) };
    static YIELD_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Live interpreter frame depth. `0` before the first frame.
pub(crate) fn frame_depth() -> u8 {
    FRAME_DEPTH.with(Cell::get)
}

pub(crate) fn clear_frame_depth() {
    FRAME_DEPTH.with(|c| c.set(0));
}

fn note_frame_depth<EVM>(evm: &mut EVM)
where
    EVM: EvmTr<Frame = EthFrame<EthInterpreter>>,
{
    let depth = evm.frame_stack().get().depth.min(u8::MAX as usize) as u8;
    FRAME_DEPTH.with(|c| c.set(depth));
}

/// The host read is returning YieldWait. `Handler::run` must not discard first.
pub(crate) fn flag_yield_wait() {
    YIELD_PENDING.with(|c| c.set(true));
}

pub(crate) fn take_yield_pending() -> bool {
    YIELD_PENDING.with(|c| c.replace(false))
}

/// Nested re-entry guard. The in-host wait does not re-enter the read.
pub(crate) fn in_yield_reread() -> bool {
    false
}

fn prep_yield_resume<EVM>(evm: &mut EVM) -> bool
where
    EVM: EvmTr<Context: ContextTr, Frame = EthFrame<EthInterpreter>>,
{
    if evm.frame_stack().index().is_none() {
        return false;
    }
    *evm.ctx().error() = Ok(());
    let frame = evm.frame_stack().get();
    if frame.is_finished() {
        return false;
    }
    // Halt left the PC one byte past the opcode and `continue_execution` clear.
    // Rewind that byte and clear the halt so the same opcode runs again.
    *frame.interpreter.bytecode.action() = None;
    frame.interpreter.bytecode.reset_action();
    frame.interpreter.bytecode.relative_jump(-1);
    true
}

pub(crate) fn run_ethereum_tx<DB: Database>(
    evm: &mut <PevmEthereum as PevmChain>::Evm<DB>,
    use_inspect: bool,
) -> Result<ExecutionResult<HaltReason>, EthDbError<DB>> {
    let mut h =
        NoBeneficiaryHandler::<<PevmEthereum as PevmChain>::Evm<DB>, EthDbError<DB>>::default();
    if use_inspect {
        h.inspect_run(evm)
    } else {
        h.run(evm)
    }
}
