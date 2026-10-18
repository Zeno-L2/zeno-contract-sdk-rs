//! The call context.

use crate::Event;
use crate::abort::{abort, codes};
use crate::effects;
use crate::types::{
    AssetId, Caller, CodeHash, ContractId, Deposit, Hash32, L1Address, Momentum, U256,
};

/// The state of the call in progress: the frame fields and the deposit
/// claim. A contract receives it as the first argument of each method.
pub struct Ctx {
    chain_id: Hash32,
    caller: Caller,
    contract_id: ContractId,
    code_hash: CodeHash,
    momentum: Momentum,
    input_index: u64,
    deposit: Deposit,
    pub(crate) claimable: U256,
    pub(crate) claimed: U256,
}

impl Ctx {
    pub(crate) fn new(
        chain_id: Hash32,
        caller: Caller,
        contract_id: ContractId,
        code_hash: CodeHash,
        momentum: Momentum,
        input_index: u64,
        deposit: Deposit,
    ) -> Ctx {
        Ctx {
            chain_id,
            caller,
            contract_id,
            code_hash,
            momentum,
            input_index,
            deposit,
            claimable: U256::ZERO,
            claimed: U256::ZERO,
        }
    }

    /// The account that sent the call.
    pub fn caller(&self) -> Caller {
        self.caller
    }

    /// This contract's id.
    pub fn contract_id(&self) -> ContractId {
        self.contract_id
    }

    /// The id of the chain the call runs on.
    pub fn chain_id(&self) -> Hash32 {
        self.chain_id
    }

    /// The hash of this contract's code.
    pub fn code_hash(&self) -> CodeHash {
        self.code_hash
    }

    /// The momentum the call executes in.
    pub fn momentum(&self) -> Momentum {
        self.momentum
    }

    /// The index of the input that carries this call.
    pub fn input_index(&self) -> u64 {
        self.input_index
    }

    /// The deposit attached to the call, or `None` when its amount is
    /// zero.
    pub fn deposit(&self) -> Option<Deposit> {
        if self.deposit.amount.is_zero() {
            None
        } else {
            Some(self.deposit)
        }
    }

    /// Claims `amount` of the deposit; a second claim replaces the
    /// first. Aborts -5 when `amount` is above the claimable amount,
    /// which is the deposit in a deposit handler or a `payable` method
    /// and zero elsewhere.
    pub fn claim_deposit(&mut self, amount: U256) {
        if amount > self.claimable {
            abort(codes::CLAIM_ABOVE_DEPOSIT);
        }
        self.claimed = amount;
    }

    /// Records `event`. Aborts -4 when it breaks a cap or does not fit.
    pub fn emit<E: Event>(&mut self, event: E) {
        let mut indexed = [[0u8; 32]; 3];
        let n = event.topics(&mut indexed);
        let mut topics = [E::TOPIC; 4];
        topics[1..=n].copy_from_slice(&indexed[..n]);
        if effects::with(|e| e.event(&topics[..=n], |w| event.encode_data(w))).is_err() {
            abort(codes::CAP_EXCEEDED);
        }
    }

    /// Registers a withdrawal of `amount` of `asset` to the L1 address
    /// `to`. Aborts -9 unless `to` is a user address, then -4 when the
    /// withdrawal breaks a cap or does not fit.
    pub fn withdraw(&mut self, to: L1Address, asset: AssetId, amount: U256) {
        if !to.is_user() {
            abort(codes::INVALID_RECIPIENT);
        }
        let added =
            effects::with(|e| e.withdrawal(to.to_bytes(), asset.to_bytes(), amount.to_be_bytes()));
        if added.is_err() {
            abort(codes::CAP_EXCEEDED);
        }
    }
}
