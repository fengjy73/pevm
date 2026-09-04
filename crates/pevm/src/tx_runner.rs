//! Shared no-beneficiary Handler for pevm execute paths.

use revm::{
    Database,
    context::{
        ContextTr, JournalTr,
        result::{EVMError, ExecutionResult, HaltReason, InvalidTransaction},
    },
    handler::{EthFrame, EvmTr, EvmTrError, FrameResult, Handler},
    inspector::{InspectorEvmTr, InspectorHandler, JournalExt},
    interpreter::interpreter::EthInterpreter,
    state::EvmState,
    Inspector,
};

use crate::chain::{PevmChain, PevmEthereum};

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
    EVM: EvmTr<Context: ContextTr<Journal: JournalTr<State = EvmState>>, Frame = EthFrame<EthInterpreter>>,
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
    let mut h = NoBeneficiaryHandler::<<PevmEthereum as PevmChain>::Evm<DB>, EthDbError<DB>>::default();
    if use_inspect {
        h.inspect_run(evm)
    } else {
        h.run(evm)
    }
}
