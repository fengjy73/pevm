//! Shared no-beneficiary Handler for pevm execute paths.

use revm::{
    Database, Inspector,
    context::{
        ContextTr, JournalTr,
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction},
    },
    context_interface::context::take_error,
    handler::{
        EthFrame, EvmTr, EvmTrError, FrameResult, FrameTr, Handler, ItemOrResult,
        instructions::InstructionProvider,
    },
    inspector::{InspectorEvmTr, InspectorHandler, JournalExt},
    interpreter::{
        Host, InitialAndFloorGas, InstructionResult, InterpreterAction,
        interpreter::EthInterpreter,
        interpreter_action::FrameInit,
        interpreter_types::Jumps,
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
        Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt> + Host,
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<
            Context = <EVM as EvmTr>::Context,
            InterpreterTypes = EthInterpreter,
        >,
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

    /// Skip `catch_error` when the interpreter was rewound and must outlive
    /// `Blocking`. `Vm` moves that `Evm` aside before the next `set_tx`.
    fn run(
        &mut self,
        evm: &mut Self::Evm,
    ) -> Result<ExecutionResult<Self::HaltReason>, Self::Error> {
        match self.run_without_catch_error(evm) {
            Ok(output) => Ok(output),
            Err(e) => {
                if crate::specfence::frame_suspend::is_held() {
                    Err(e)
                } else {
                    self.catch_error(evm, e)
                }
            }
        }
    }

    fn run_without_catch_error(
        &mut self,
        evm: &mut Self::Evm,
    ) -> Result<ExecutionResult<Self::HaltReason>, Self::Error> {
        let mut init_and_floor_gas = self.validate(evm)?;
        let eip7702_refund = self.pre_execution(evm, &mut init_and_floor_gas)? as i64;
        let mut exec_result = match self.execution(evm, &init_and_floor_gas) {
            Ok(result) => result,
            Err(e) => {
                if crate::specfence::frame_suspend::is_held() {
                    crate::specfence::frame_suspend::stash_gas(init_and_floor_gas, eip7702_refund);
                }
                return Err(e);
            }
        };
        let result_gas =
            self.post_execution(evm, &mut exec_result, init_and_floor_gas, eip7702_refund)?;
        self.execution_result(evm, exec_result, result_gas)
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

        pump_frames::<EVM, ERROR>(evm)
    }
}

/// Frame loop shared by the first `Handler::run` and a same-frame resume.
fn pump_frames<EVM, ERROR>(evm: &mut EVM) -> Result<FrameResult, ERROR>
where
    EVM: EvmTr<
        Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt> + Host,
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<
            Context = <EVM as EvmTr>::Context,
            InterpreterTypes = EthInterpreter,
        >,
    >,
    ERROR: EvmTrError<EVM>,
{
    loop {
        let call_or_result = drive_top_frame::<EVM, ERROR>(evm)?;

        let result = match call_or_result {
            ItemOrResult::Item(init) => match evm.frame_init(init)? {
                ItemOrResult::Item(_) => {
                    // Iter29: hang-free nested OrderedAdmit consume after natural CALL
                    // enters a new frame — stash is consulted only here (PENDING
                    // cleared on mismatch; ≠ Iter28e frame_init-defer).
                    if nested_ordered_admit_stash_armed() {
                        let evm_ptr = evm as *mut EVM;
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

/// One top-frame burst. After `run_plain`, a rewind-safe `basic` can hold
/// the frame instead of finishing it.
fn drive_top_frame<EVM, ERROR>(evm: &mut EVM) -> Result<ItemOrResult<FrameInit, FrameResult>, ERROR>
where
    EVM: EvmTr<
        Context: ContextTr + Host,
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<
            Context = <EVM as EvmTr>::Context,
            InterpreterTypes = EthInterpreter,
        >,
    >,
    ERROR: EvmTrError<EVM>,
{
    let action = run_plain_stock(evm);
    let action = hold_if_rewind_safe::<EVM, ERROR>(evm, action)?;
    apply_interpreter_action(evm, action)
}

fn run_plain_stock<EVM>(evm: &mut EVM) -> InterpreterAction
where
    EVM: EvmTr<
        Context: ContextTr + Host,
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<
            Context = <EVM as EvmTr>::Context,
            InterpreterTypes = EthInterpreter,
        >,
    >,
{
    let (ctx, instructions, _, frames) = evm.all_mut();
    let table = instructions.instruction_table();
    frames.get().interpreter.run_plain(table, ctx)
}

/// `run_plain` already stepped past the opcode and charged static gas.
/// Rewind only when that opcode did not pop or resize before `basic`.
fn hold_if_rewind_safe<EVM, ERROR>(
    evm: &mut EVM,
    action: InterpreterAction,
) -> Result<InterpreterAction, ERROR>
where
    EVM: EvmTr<
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<InterpreterTypes = EthInterpreter, Context: Host>,
    >,
    ERROR: EvmTrError<EVM>,
{
    if action.instruction_result() != Some(InstructionResult::FatalExternalError) {
        return Ok(action);
    }
    let Some(pred) = crate::specfence::frame_suspend::take_request() else {
        return Ok(action);
    };
    let op_safe = {
        let (_, instructions, _, frames) = evm.all_mut();
        let interp = &mut frames.get().interpreter;
        interp.bytecode.relative_jump(-1);
        let op = interp.bytecode.opcode();
        if !crate::specfence::frame_suspend::opcode_rewind_safe(op) {
            interp.bytecode.relative_jump(1);
            crate::specfence::frame_suspend::note_unsafe_opcode(op);
            false
        } else {
            let static_gas = instructions.instruction_table()[op as usize].static_gas();
            interp.gas.erase_cost(static_gas);
            true
        }
    };
    if !op_safe {
        return Ok(action);
    }
    match take_error::<ERROR, _>(evm.ctx().error()) {
        Err(e) => {
            crate::specfence::frame_suspend::mark_held(pred);
            Err(e)
        }
        Ok(()) => Ok(action),
    }
}

fn apply_interpreter_action<EVM, ERROR>(
    evm: &mut EVM,
    action: InterpreterAction,
) -> Result<ItemOrResult<FrameInit, FrameResult>, ERROR>
where
    EVM: EvmTr<Context: ContextTr, Frame = EthFrame<EthInterpreter>>,
    ERROR: EvmTrError<EVM>,
{
    let processed = {
        let (ctx, _, _, frames) = evm.all_mut();
        frames
            .get()
            .process_next_action::<<EVM as EvmTr>::Context, ERROR>(ctx, action)?
    };
    if processed.is_result() {
        evm.frame_stack().get().set_finished(true);
    }
    Ok(processed)
}

impl<EVM, ERROR> NoBeneficiaryHandler<EVM, ERROR>
where
    EVM: EvmTr<
        Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt> + Host,
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<
            Context = <EVM as EvmTr>::Context,
            InterpreterTypes = EthInterpreter,
        >,
    >,
    ERROR: EvmTrError<EVM>,
{
    /// Continue a frame that already survived `Blocking`. No `validate`,
    /// `pre_execution`, or `frame_init` of the suspended root.
    fn resume_held(
        &mut self,
        evm: &mut EVM,
        init_and_floor_gas: InitialAndFloorGas,
        eip7702_refund: i64,
    ) -> Result<ExecutionResult<HaltReason>, ERROR> {
        crate::specfence::frame_suspend::note_resume();
        let mut exec_result = match pump_frames::<EVM, ERROR>(evm) {
            Ok(result) => result,
            Err(e) => {
                if crate::specfence::frame_suspend::is_held() {
                    crate::specfence::frame_suspend::stash_gas(init_and_floor_gas, eip7702_refund);
                    return Err(e);
                }
                return self.catch_error(evm, e);
            }
        };
        self.last_frame_result(evm, &mut exec_result)?;
        let result_gas =
            self.post_execution(evm, &mut exec_result, init_and_floor_gas, eip7702_refund)?;
        self.execution_result(evm, exec_result, result_gas)
    }
}

impl<EVM, ERROR> InspectorHandler for NoBeneficiaryHandler<EVM, ERROR>
where
    EVM: InspectorEvmTr<
        Context: ContextTr<Journal: JournalTr<State = EvmState> + JournalExt> + Host,
        Frame = EthFrame<EthInterpreter>,
        Instructions: InstructionProvider<
            Context = <EVM as EvmTr>::Context,
            InterpreterTypes = EthInterpreter,
        >,
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

/// Continue the suspended root frame. Not [`Handler::run`].
pub(crate) fn resume_ethereum_tx<DB: Database>(
    evm: &mut <PevmEthereum as PevmChain>::Evm<DB>,
    init: InitialAndFloorGas,
    eip7702_refund: i64,
) -> Result<ExecutionResult<HaltReason>, EthDbError<DB>> {
    let mut h =
        NoBeneficiaryHandler::<<PevmEthereum as PevmChain>::Evm<DB>, EthDbError<DB>>::default();
    h.resume_held(evm, init, eip7702_refund)
}
