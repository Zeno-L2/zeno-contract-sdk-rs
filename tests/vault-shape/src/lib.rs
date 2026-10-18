//! The vault declarations in macro form, plus one method with borrowed
//! arguments and cfg-gated items, so argument decoding, the deposit
//! handler, the payable arm and feature gating all go through the
//! macros.
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
        /// The recipient is not a user address.
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

    /// A withdrawal debited from the account's balance.
    #[event]
    pub struct Withdrawn {
        #[indexed]
        pub account: Caller,
        #[indexed]
        pub asset: AssetId,
        pub amount: U256,
        pub l1_recipient: L1Address,
    }

    /// An event that is compiled out.
    #[cfg(any())]
    #[event]
    pub struct Hidden {
        #[indexed]
        pub account: Caller,
    }

    /// Credits the attached deposit to the caller's balance.
    #[on_deposit]
    pub fn deposit(ctx: &mut Ctx) -> Result<(), VaultError> {
        let Some(attached) = ctx.deposit() else {
            return Ok(());
        };
        let account = ctx.caller();
        let key = (account, attached.asset);
        let balance = BALANCES
            .may_load(ctx, &key)
            .unwrap_or(U256::ZERO)
            .checked_add(attached.amount)
            .ok_or(VaultError::BalanceOverflow)?;
        BALANCES.save(ctx, &key, &balance);
        ctx.emit(Deposited {
            account,
            asset: attached.asset,
            amount: attached.amount,
        });
        ctx.claim_deposit(attached.amount);
        Ok(())
    }

    /// Debits the caller's balance of the asset and registers a
    /// withdrawal.
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

    /// A method that is compiled out.
    #[cfg(any())]
    #[method(payable)]
    pub fn claim_partial(ctx: &mut Ctx, amount: U256) {
        ctx.claim_deposit(amount);
    }

    /// Takes borrowed arguments and claims any attached deposit.
    #[method(payable)]
    pub fn echo(ctx: &mut Ctx, data: &[u8], note: &str) {
        let _ = (data, note);
        if let Some(attached) = ctx.deposit() {
            ctx.claim_deposit(attached.amount);
        }
    }
}

#[cfg(test)]
mod interface_golden {
    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const GOLDEN: &str = include_str!("../golden/vault-shape.interface.json");

    #[test]
    fn interface_text_matches_the_golden_file() {
        assert_eq!(super::vault::__zeno_interface().to_json(ZERO_HASH), GOLDEN);
    }
}
