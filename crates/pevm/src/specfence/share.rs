//! Shared bytecode and base-state cache for the parallel path.
//!
//! `SPECFENCE_SHARED_CODE=0` keeps the per-worker code cache. `=1`, or an
//! unset variable, analyzes each code hash once per block and shares the
//! `Bytecode` (`Arc` inside revm) across workers.
//!
//! `SPECFENCE_SHARED_CACHE=0` keeps the per-worker account and slot maps.
//! `=1`, or an unset variable, uses one read-mostly cache, sharded so fills
//! do not share a lock, and prefilled from the block's caller, callee, and
//! beneficiary.

use std::sync::{OnceLock, RwLock};

use alloy_primitives::{Address, B256, U256};
use dashmap::DashMap;
use hashbrown::HashMap;
use revm::state::Bytecode;
use rustc_hash::FxBuildHasher;

use crate::{AccountBasic, BuildSuffixHasher, Storage, chain::PevmChain};

const SHARDS: usize = 32;

fn flag(name: &str) -> bool {
    // Default on. Experiments pass `=0` for the off leg. OnceLock so the
    // hot path does not read the environment.
    std::env::var(name).ok().as_deref() != Some("0")
}

pub(crate) fn code_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag("SPECFENCE_SHARED_CODE"))
}

pub(crate) fn cache_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag("SPECFENCE_SHARED_CACHE"))
}

#[repr(align(64))]
struct Shard<K, V> {
    map: RwLock<HashMap<K, V, FxBuildHasher>>,
}

impl<K, V> Shard<K, V> {
    const fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::with_hasher(FxBuildHasher)),
        }
    }
}

pub(crate) struct CodeShare {
    map: DashMap<B256, Bytecode, BuildSuffixHasher>,
}

impl CodeShare {
    pub(crate) fn new() -> Self {
        Self {
            map: DashMap::with_hasher(BuildSuffixHasher::default()),
        }
    }

    pub(crate) fn get(&self, hash: &B256) -> Option<Bytecode> {
        let _wait = super::buckets::WaitGuard::start(super::buckets::WAIT_LOCK);
        self.map.get(hash).map(|code| code.clone())
    }

    /// Keep the first analyzed value. A later insert of the same hash is a clone.
    pub(crate) fn insert(&self, hash: B256, code: Bytecode) {
        self.map.entry(hash).or_insert(code);
    }
}

pub(crate) struct BaseShare {
    basic: Vec<Shard<Address, Option<AccountBasic>>>,
    code_hash: Vec<Shard<Address, Option<B256>>>,
    slots: Vec<Shard<(Address, U256), U256>>,
}

impl BaseShare {
    pub(crate) fn new() -> Self {
        Self {
            basic: (0..SHARDS).map(|_| Shard::new()).collect(),
            code_hash: (0..SHARDS).map(|_| Shard::new()).collect(),
            slots: (0..SHARDS).map(|_| Shard::new()).collect(),
        }
    }

    pub(crate) fn lookup_basic(&self, address: &Address) -> Option<Option<AccountBasic>> {
        let _wait = super::buckets::WaitGuard::start(super::buckets::WAIT_LOCK);
        let guard = self.basic[shard_addr(address)].map.read().unwrap();
        guard.get(address).cloned()
    }

    pub(crate) fn insert_basic(&self, address: Address, value: Option<AccountBasic>) {
        let mut guard = self.basic[shard_addr(&address)].map.write().unwrap();
        guard.entry(address).or_insert(value);
    }

    pub(crate) fn lookup_code_hash(&self, address: &Address) -> Option<Option<B256>> {
        let _wait = super::buckets::WaitGuard::start(super::buckets::WAIT_LOCK);
        let guard = self.code_hash[shard_addr(address)].map.read().unwrap();
        guard.get(address).copied()
    }

    pub(crate) fn insert_code_hash(&self, address: Address, value: Option<B256>) {
        let mut guard = self.code_hash[shard_addr(&address)].map.write().unwrap();
        guard.entry(address).or_insert(value);
    }

    pub(crate) fn lookup_slot(&self, address: &Address, index: &U256) -> Option<U256> {
        let _wait = super::buckets::WaitGuard::start(super::buckets::WAIT_LOCK);
        let key = (*address, *index);
        let shard = (address.0[0] as usize ^ address.0[19] as usize) % SHARDS;
        let guard = self.slots[shard].map.read().unwrap();
        guard.get(&key).copied()
    }

    pub(crate) fn insert_slot(&self, address: Address, index: U256, value: U256) {
        let shard = (address.0[0] as usize ^ address.0[19] as usize) % SHARDS;
        let mut guard = self.slots[shard].map.write().unwrap();
        guard.entry((address, index)).or_insert(value);
    }
}

fn shard_addr(address: &Address) -> usize {
    address.0[0] as usize % SHARDS
}

/// Load caller, callee, and beneficiary accounts before workers start.
pub(crate) fn prefill<S, C>(
    chain: &C,
    storage: &S,
    beneficiary: Address,
    txs: &[C::EvmTx],
    codes: &CodeShare,
    base: &BaseShare,
) where
    S: Storage,
    C: PevmChain,
{
    let share_code = code_on();
    let share_base = cache_on();
    if !share_code && !share_base {
        return;
    }
    let mut addrs = Vec::with_capacity(txs.len().saturating_mul(2).saturating_add(1));
    addrs.push(beneficiary);
    for tx in txs {
        let env = chain.tx_env(tx);
        addrs.push(env.caller);
        if let Some(to) = env.kind.to() {
            addrs.push(*to);
        }
    }
    addrs.sort_unstable();
    addrs.dedup();
    for address in addrs {
        if share_base {
            if let Ok(basic) = storage.basic(&address) {
                base.insert_basic(address, basic);
            }
            if let Ok(hash) = storage.code_hash(&address) {
                base.insert_code_hash(address, hash);
                if share_code
                    && let Some(hash) = hash
                    && codes.get(&hash).is_none()
                    && let Ok(Some(evm)) = storage.code_by_hash(&hash)
                {
                    codes.insert(hash, Bytecode::from(evm));
                }
            }
        } else if share_code
            && let Ok(Some(hash)) = storage.code_hash(&address)
            && codes.get(&hash).is_none()
            && let Ok(Some(evm)) = storage.code_by_hash(&hash)
        {
            codes.insert(hash, Bytecode::from(evm));
        }
    }
}
