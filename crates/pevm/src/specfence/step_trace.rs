//! Measurement-only access offsets for Ideal_step.
//!
//! Off unless `SPECFENCE_STEP_TRACE=1`. Flag-off call sites do not take
//! `Instant` and do not enter `inspect_run`. The trace run does: the
//! inspector only records PC / opcode index and returns, so it does not arm
//! jumps. Offsets are nanoseconds from the same `ExecPhase` start that the
//! per-tx sequential cost uses. Walls from a trace run are not product walls.

#![allow(missing_docs)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use alloy_primitives::{Address, B256, U256};
use revm::interpreter::{
    Interpreter,
    interpreter::EthInterpreter,
    interpreter_types::{InputsTr, Jumps, LegacyBytecode, StackTr},
};
use serde::Serialize;

use crate::{MemoryLocation, hash_deterministic};

const KIND_BASIC: u8 = 0;
const KIND_STORAGE: u8 = 1;
const KIND_CODE: u8 = 2;
const KIND_LAZY: u8 = 3;

const RW_READ: u8 = 0;
const RW_WRITE: u8 = 1;

pub const fn kind_basic() -> u8 {
    KIND_BASIC
}
pub const fn kind_storage() -> u8 {
    KIND_STORAGE
}
pub const fn kind_code() -> u8 {
    KIND_CODE
}
pub const fn kind_lazy() -> u8 {
    KIND_LAZY
}

fn env_on() -> bool {
    matches!(
        std::env::var("SPECFENCE_STEP_TRACE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

#[inline]
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(env_on)
}

#[derive(Clone, Serialize)]
pub struct Access {
    pub ns: u64,
    pub pc: u32,
    pub op_index: u32,
    pub op: u8,
    pub code_hash: String,
    pub loc: u64,
    pub kind: u8,
    pub rw: u8,
    pub addr: String,
    pub slot: String,
}

#[derive(Clone, Serialize)]
pub struct TxTrace {
    pub tx: u32,
    pub inc: u16,
    pub dur_ns: u64,
    pub selector: String,
    pub accesses: Vec<Access>,
}

struct Pending {
    loc: u64,
    kind: u8,
    addr: Address,
    slot: U256,
    op: u8,
}

struct Cur {
    tx: u32,
    inc: u16,
    t0: Option<Instant>,
    selector: [u8; 4],
    pc: u32,
    /// Next opcode index. `op_index_live` is the 0-based index of the opcode
    /// currently executing.
    op_index: u32,
    op_index_live: u32,
    op: u8,
    code_hash: B256,
    frame: Address,
    pending: Vec<Pending>,
    wrote: Vec<u64>,
}

impl Cur {
    fn clear(&mut self) {
        self.t0 = None;
        self.selector = [0; 4];
        self.pc = 0;
        self.op_index = 0;
        self.op_index_live = 0;
        self.op = 0;
        self.code_hash = B256::ZERO;
        self.frame = Address::ZERO;
        self.pending.clear();
        self.wrote.clear();
    }
}

thread_local! {
    static CUR: RefCell<Cur> = RefCell::new(Cur {
        tx: 0,
        inc: 0,
        t0: None,
        selector: [0; 4],
        pc: 0,
        op_index: 0,
        op_index_live: 0,
        op: 0,
        code_hash: B256::ZERO,
        frame: Address::ZERO,
        pending: Vec::new(),
        wrote: Vec::new(),
    });
}

fn traces() -> &'static Mutex<Vec<TxTrace>> {
    static TRACES: OnceLock<Mutex<Vec<TxTrace>>> = OnceLock::new();
    TRACES.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn begin_tx(tx: usize, inc: u16, t0: Instant) {
    if !enabled() {
        return;
    }
    CUR.with(|c| {
        let mut c = c.borrow_mut();
        c.clear();
        c.tx = tx as u32;
        c.inc = inc;
        c.t0 = Some(t0);
    });
    ensure_slot();
}

pub fn set_selector(selector: [u8; 4]) {
    if !enabled() {
        return;
    }
    CUR.with(|c| c.borrow_mut().selector = selector);
}

pub fn end_tx(dur_ns: u64) {
    if !enabled() {
        return;
    }
    let (tx, inc, selector) = CUR.with(|c| {
        let mut c = c.borrow_mut();
        let out = (c.tx, c.inc, hex_prefix(&c.selector));
        c.t0 = None;
        out
    });
    let mut t = traces().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = t
        .iter_mut()
        .rev()
        .find(|s| s.tx == tx && s.inc == inc && s.dur_ns == 0)
    {
        slot.dur_ns = dur_ns;
        slot.selector = selector;
    }
}

fn hex_prefix(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn addr_hex(addr: Address) -> String {
    hex_prefix(addr.as_slice())
}

fn slot_hex(slot: U256) -> String {
    hex_prefix(&slot.to_be_bytes::<32>())
}

fn ensure_slot() {
    let (tx, inc) = CUR.with(|c| {
        let c = c.borrow();
        (c.tx, c.inc)
    });
    let mut t = traces().lock().unwrap_or_else(|e| e.into_inner());
    let open = t
        .iter()
        .rev()
        .any(|s| s.tx == tx && s.inc == inc && s.dur_ns == 0);
    if !open {
        t.push(TxTrace {
            tx,
            inc,
            dur_ns: 0,
            selector: String::new(),
            accesses: Vec::new(),
        });
    }
}

fn push_access(loc: u64, kind: u8, rw: u8, addr: Address, slot: U256, op: u8, host: bool) {
    ensure_slot();
    let (ns, pc, op_index, code, op) = CUR.with(|c| {
        let c = c.borrow();
        let ns = c.t0.map(|t| t.elapsed().as_nanos() as u64).unwrap_or(0);
        if host {
            // Post-interpreter settlement has no opcode. Do not inherit the
            // last frame's pc / code hash.
            (ns, 0, 0, String::new(), 0)
        } else {
            (
                ns,
                c.pc,
                c.op_index_live,
                hex_prefix(c.code_hash.as_slice()),
                op,
            )
        }
    });
    let access = Access {
        ns,
        pc,
        op_index,
        op,
        code_hash: code,
        loc,
        kind,
        rw,
        addr: addr_hex(addr),
        slot: if kind == KIND_STORAGE {
            slot_hex(slot)
        } else {
            String::new()
        },
    };
    let (tx, inc) = CUR.with(|c| {
        let c = c.borrow();
        (c.tx, c.inc)
    });
    let mut t = traces().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = t.iter_mut().rev().find(|s| s.tx == tx && s.inc == inc) {
        if rw == RW_READ
            && slot
                .accesses
                .iter()
                .any(|a| a.loc == loc && a.rw == RW_READ)
        {
            return;
        }
        slot.accesses.push(access);
    }
}

pub fn note_read(loc: u64, kind: u8, addr: Address) {
    if !enabled() {
        return;
    }
    let op = CUR.with(|c| c.borrow().op);
    push_access(loc, kind, RW_READ, addr, U256::ZERO, op, false);
}

pub fn note_write_if_missing(loc: u64, kind: u8, force: bool) {
    if !enabled() {
        return;
    }
    let already = CUR.with(|c| c.borrow().wrote.contains(&loc));
    // `force` records the post-interpreter settlement (sender gas/nonce,
    // beneficiary reward) even when an earlier opcode already wrote the
    // location. Storage keeps the SSTORE timestamp unless it was never seen.
    if already && !force {
        return;
    }
    if !already {
        CUR.with(|c| c.borrow_mut().wrote.push(loc));
    }
    push_access(loc, kind, RW_WRITE, Address::ZERO, U256::ZERO, 0, true);
}

/// Inspector `step`. Returns true when the trace owns the callback and the
/// product inspector body must not run.
pub fn on_step(interp: &mut Interpreter<EthInterpreter>) -> bool {
    if !enabled() {
        return false;
    }
    let pc = interp.bytecode.pc() as u32;
    let op = interp
        .bytecode
        .bytecode_slice()
        .get(pc as usize)
        .copied()
        .unwrap_or(0);
    let frame = interp.input.target_address();
    CUR.with(|c| {
        let mut c = c.borrow_mut();
        c.op_index_live = c.op_index;
        c.op_index = c.op_index.saturating_add(1);
        c.pc = pc;
        c.op = op;
        if c.frame != frame {
            c.code_hash = interp.bytecode.get_or_calculate_hash();
            c.frame = frame;
        }
    });
    note_opcode_write(interp, op);
    true
}

pub fn on_step_end() -> bool {
    if !enabled() {
        return false;
    }
    let pending = CUR.with(|c| std::mem::take(&mut c.borrow_mut().pending));
    for p in pending {
        CUR.with(|c| {
            if !c.borrow().wrote.contains(&p.loc) {
                c.borrow_mut().wrote.push(p.loc);
            }
        });
        push_access(p.loc, p.kind, RW_WRITE, p.addr, p.slot, p.op, false);
    }
    true
}

fn note_opcode_write(interp: &Interpreter<EthInterpreter>, op: u8) {
    const OP_SSTORE: u8 = 0x55;
    const OP_CALL: u8 = 0xf1;
    const OP_CALLCODE: u8 = 0xf2;
    const OP_CREATE: u8 = 0xf0;
    const OP_CREATE2: u8 = 0xf5;
    const OP_SELFDESTRUCT: u8 = 0xff;
    let target = interp.input.target_address();
    match op {
        OP_SSTORE => {
            // Stack top is value, peek(1) is key, before the opcode runs.
            let Ok(key) = interp.stack.peek(1) else {
                return;
            };
            let loc = hash_deterministic(MemoryLocation::Storage(target, key));
            CUR.with(|c| {
                c.borrow_mut().pending.push(Pending {
                    loc,
                    kind: KIND_STORAGE,
                    addr: target,
                    slot: key,
                    op,
                });
            });
        }
        OP_CALL | OP_CALLCODE => {
            let Ok(value) = interp.stack.peek(2) else {
                return;
            };
            if value.is_zero() {
                return;
            }
            let Ok(addr_u) = interp.stack.peek(1) else {
                return;
            };
            let to = u256_addr(addr_u);
            let caller = interp.input.caller_address();
            // Stamp at opcode entry. `step_end` runs after the callee returns,
            // which is later than the value transfer.
            commit_basic(caller, op);
            commit_basic(to, op);
        }
        OP_CREATE | OP_CREATE2 => {
            commit_basic(interp.input.caller_address(), op);
        }
        OP_SELFDESTRUCT => {
            commit_basic(target, op);
            if let Ok(addr_u) = interp.stack.peek(0) {
                commit_basic(u256_addr(addr_u), op);
            }
        }
        _ => {}
    }
}

fn commit_basic(addr: Address, op: u8) {
    let loc = hash_deterministic(MemoryLocation::Basic(addr));
    CUR.with(|c| {
        let mut c = c.borrow_mut();
        if !c.wrote.contains(&loc) {
            c.wrote.push(loc);
        }
    });
    push_access(loc, KIND_BASIC, RW_WRITE, addr, U256::ZERO, op, false);
}

fn u256_addr(v: U256) -> Address {
    Address::from_word(B256::from(v))
}

pub fn drain() -> Vec<TxTrace> {
    let mut t = traces().lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *t)
}

static BEN: AtomicU64 = AtomicU64::new(0);

pub fn beneficiary_store(hash: u64) {
    if enabled() {
        BEN.store(hash, Ordering::Relaxed);
    }
}

pub fn beneficiary() -> u64 {
    BEN.load(Ordering::Relaxed)
}
