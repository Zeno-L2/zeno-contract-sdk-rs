//! The reference vault: holds each caller's deposited balance per asset
//! and pays it out to an L1 user address on `withdraw`. It is example
//! code to copy for custody-holding contracts. The `test-hook` feature
//! adds `claim_partial`, which claims part of a deposit, for tests
//! only.
#![no_std]

use zeno_contract::prelude::*;

#[contract]
mod vault {
    use super::*;

    /// An absent leaf is a zero balance.
    #[storage(0x01, key(account, asset))]
    const BALANCES: Map<(Caller, AssetId), U256> = Map::new();

    #[contract_error]
    pub enum VaultError {
        /// The caller's balance of the asset is below the requested
        /// amount.
        InsufficientBalance {
            asset: AssetId,
            requested: U256,
            available: U256,
        } = 1,
        /// Crediting the deposit would overflow the u256 balance.
        BalanceOverflow = 2,
        /// The withdrawal amount is zero.
        ZeroAmount = 3,
        /// The recipient is not a user address (first byte 0x00, not
        /// all zero).
        InvalidRecipient = 4,
    }

    /// A deposit credited to the account's balance.
    #[event]
    pub struct Deposited {
        #[indexed]
        pub account: Caller,
        #[indexed]
        pub asset: AssetId,
        pub amount: U256,
    }

    /// A withdrawal debited from the account's balance and registered
    /// for the L1 recipient.
    #[event]
    pub struct Withdrawn {
        #[indexed]
        pub account: Caller,
        #[indexed]
        pub asset: AssetId,
        pub amount: U256,
        pub l1_recipient: L1Address,
    }

    /// Credits `amount` of `asset` to the caller, emits `Deposited` and
    /// claims `amount`.
    fn credit(ctx: &mut Ctx, asset: AssetId, amount: U256) -> Result<(), VaultError> {
        let account = ctx.caller();
        let key = (account, asset);
        let balance = BALANCES
            .may_load(ctx, &key)
            .unwrap_or(U256::ZERO)
            .checked_add(amount)
            .ok_or(VaultError::BalanceOverflow)?;
        BALANCES.save(ctx, &key, &balance);
        ctx.emit(Deposited {
            account,
            asset,
            amount,
        });
        ctx.claim_deposit(amount);
        Ok(())
    }

    /// Credits the attached deposit to the caller's balance.
    #[on_deposit]
    pub fn deposit(ctx: &mut Ctx) -> Result<(), VaultError> {
        let Some(attached) = ctx.deposit() else {
            return Ok(());
        };
        credit(ctx, attached.asset, attached.amount)
    }

    /// Debits the caller's balance of the asset and registers a
    /// withdrawal of the amount to the L1 recipient.
    #[method]
    pub fn withdraw(
        ctx: &mut Ctx,
        asset: AssetId,
        amount: U256,
        l1_recipient: L1Address,
    ) -> Result<(), VaultError> {
        if amount.is_zero() {
            return Err(VaultError::ZeroAmount);
        }
        if !l1_recipient.is_user() {
            return Err(VaultError::InvalidRecipient);
        }
        let account = ctx.caller();
        let key = (account, asset);
        let available = BALANCES.may_load(ctx, &key).unwrap_or(U256::ZERO);
        let Some(remaining) = available.checked_sub(amount) else {
            return Err(VaultError::InsufficientBalance {
                asset,
                requested: amount,
                available,
            });
        };
        if remaining.is_zero() {
            BALANCES.remove(ctx, &key);
        } else {
            BALANCES.save(ctx, &key, &remaining);
        }
        ctx.emit(Withdrawn {
            account,
            asset,
            amount,
            l1_recipient,
        });
        ctx.withdraw(l1_recipient, asset, amount);
        Ok(())
    }

    /// Credits and claims part of the attached deposit; the runtime
    /// refunds the rest. Test builds only.
    #[cfg(feature = "test-hook")]
    #[method(payable)]
    pub fn claim_partial(ctx: &mut Ctx, claim: U256) -> Result<(), VaultError> {
        let asset = ctx.deposit().map_or(AssetId::default(), |d| d.asset);
        credit(ctx, asset, claim)
    }
}

#[cfg(test)]
mod tests;
