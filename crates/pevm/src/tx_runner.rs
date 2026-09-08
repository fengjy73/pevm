//! Shared no-beneficiary Handler for pevm execute paths.

use revm::{
    Database,
    context::{
        ContextTr, JournalTr,
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction},
    },
    handler::{
        EthFrame, EvmTr, EvmTrError, FrameResult, FrameTr, Handler, ItemOrResult,
    },
    inspector::{InspectorEvmTr, InspectorHandler, JournalExt},
    interpreter::interpreter::EthInterpreter,
    state::EvmState,
    Inspector,
};

use crate::chain::{PevmChain, PevmEthereum};
use crate::specfence::{nested_bind_stash_armed, pending_resume_armed, try_apply_pending_pc_resume, try_consume_nested_bind_resume};

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
            let call_or_result = evm.frame_run()?;

            let result = match call_or_result {
                ItemOrResult::Item(init) => match evm.frame_init(init)? {
                    ItemOrResult::Item(_) => {
                        // Iter29: hang-free nested Bind consume after natural CALL
                        // enters a new frame — stash is consulted only here (PENDING
                        // cleared on mismatch; ≠ Iter28e frame_init-defer).
                        if nested_bind_stash_armed() {
                            let evm_ptr = evm as *mut Self::Evm;
                            unsafe {
                                let frame = (*evm_ptr).frame_stack().get();
                                let depth = frame.depth.min(u16::MAX as usize) as u16;
                                let interp = &mut frame.interpreter;
                                let ctx = (*evm_ptr).ctx();
                                try_consume_nested_bind_resume(interp, ctx, depth);
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
