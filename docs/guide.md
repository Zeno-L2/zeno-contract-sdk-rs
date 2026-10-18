# Writing a contract

This guide extends the `cargo zeno new` scaffold step by step. The snippets marked with a `guide-test` comment are compiled and run against `zeno-exec` by the `guide_snippets` test, so they build as written.

## Scaffold

```bash
cargo zeno new demo --sdk-path /path/to/zeno-contract-sdk-rs
cd demo
cargo zeno test
```

`--sdk-path` points the new crate at a checkout of this repository. Without it the crate depends on the published release tag. The name matches `[a-z][a-z0-9_]*` (error `Z0211` lists the other rules). The scaffold holds `Cargo.toml`, `src/lib.rs`, `src/tests.rs`, a `README.md`, and copies of `LICENSE` and `rust-toolchain.toml`.

## The template module

`src/lib.rs` starts with `#![no_std]` and one `#[contract]` module. Everything the contract exports lives inside that module, after `use super::*;`, which brings in the prelude.

- `#[storage(0x01)] const HIGHEST: Item<u64>` declares one stored value at namespace `0x01`.
- `#[contract_error] pub enum Error` lists the contract's own abort codes. Each variant has a code from 1 up and a `///` line that becomes its description.
- `#[event] pub struct Raised` declares an event. The doc line is its description.
- `#[method(returns(previous))] pub fn raise(ctx: &mut Ctx, value: u64) -> Result<u64, Error>` is a callable method. Its first parameter is the `Ctx`, the rest are the arguments the caller encodes, and `Ok` carries the named outputs.

The `#[contract]` module must sit in `src/lib.rs`, because the interface step compiles that file (`Z0209` otherwise).

## Storage

`Item<V>` holds one value and `Map<K, V>` holds one value per key. `Item` reads with `may_load(ctx)` and `Map` with `may_load(ctx, &key)` (an absent leaf is `None`); both write with `save` and delete with `remove`. A write is visible to a later read in the same call. Values and key fields are fixed-width types (see `docs/api.md`).

Each entry takes its own namespace from `0x01` to `0xff`. A map names its key fields: `key(account, asset)` names the two tuple elements. A key of more than 30 bytes in total is hashed, so a `(Caller, AssetId)` key (64 bytes) is hashed and the interface file says `hashed: true`.

Add this map to the module. It keeps a running total per account and asset.

<!-- guide-test: module -->
```rust
    /// The total credited to an account, per asset. An absent leaf is 0.
    #[storage(0x02, key(account, asset))]
    const CREDITS: Map<(Caller, AssetId), U256> = Map::new();
```

## Errors

A contract error aborts the call with its code, a revert status and the variant's fields as revert data. The runtime discards the call's writes, events and withdrawals.

Add these variants after `NotHigher`.

<!-- guide-test: errors -->
```rust
        /// Crediting the deposit would overflow the total.
        Overflow = 2,
        /// The balance is below the requested amount.
        InsufficientCredit = 3,
        /// The withdrawal amount is zero.
        ZeroAmount = 4,
        /// The recipient is not a user address.
        InvalidRecipient = 5,
```

Codes below zero belong to the SDK (`docs/api.md`). Your codes start at 1.

## Events

`ctx.emit(event)` records an event. The event's topics are the hash of its name followed by its `#[indexed]` fields, in order. The other fields are its data. An event has at most 3 indexed fields, each 32 bytes wide.

<!-- guide-test: module -->
```rust
    /// A deposit was credited to an account.
    #[event]
    pub struct Credited {
        #[indexed]
        pub account: Caller,
        #[indexed]
        pub asset: AssetId,
        pub amount: U256,
    }
```

## Deposits

Value arrives as a deposit attached to a call. `ctx.deposit()` returns it, or `None` when its amount is zero. The contract holds no balance of its own: it keeps a ledger in storage and sends value out with `ctx.withdraw`. The runtime does not stop a withdrawal above what the contract received.

- A method marked `#[method(payable)]` may claim the deposit. A plain `#[method]` claims nothing, so the runtime refunds an attached deposit in full.
- `ctx.claim_deposit(amount)` claims `amount`. A claim above the deposit aborts -5. A second claim replaces the first.
- The runtime refunds the part of the deposit that the call does not claim. A failed call refunds all of it.
- `#[on_deposit]` marks the handler for an empty payload. Without one, an empty payload aborts -3.

A claimed deposit stays in the contract until a `ctx.withdraw` sends it out. The method below credits the full deposit to a ledger entry. The reference vault in `examples/vault` is the complete custody pattern: it also pays balances out.

<!-- guide-test: module -->
```rust
    /// Credits the attached deposit to the caller and claims all of it.
    #[method(payable)]
    pub fn credit(ctx: &mut Ctx) -> Result<(), Error> {
        let Some(attached) = ctx.deposit() else {
            return Ok(());
        };
        let account = ctx.caller();
        let key = (account, attached.asset);
        let total = CREDITS
            .may_load(ctx, &key)
            .unwrap_or(U256::ZERO)
            .checked_add(attached.amount)
            .ok_or(Error::Overflow)?;
        CREDITS.save(ctx, &key, &total);
        ctx.emit(Credited {
            account,
            asset: attached.asset,
            amount: attached.amount,
        });
        ctx.claim_deposit(attached.amount);
        Ok(())
    }
```

## Withdrawals

`ctx.withdraw(to, asset, amount)` registers a withdrawal of `amount` of `asset` to the L1 address `to`. The call returns normally; the runtime pays the withdrawal when the call succeeds.

The recipient must be a user address: the first byte is `0x00` and the address is not all zero. `ctx.withdraw` aborts -9 on any other address. To return your own error code instead, test `to.is_user()` first, as the reference vault does with `InvalidRecipient`. Do that check before any state-dependent check, so the error does not depend on the ledger.

The runtime does not limit a withdrawal to what the contract received. The contract's own ledger is the only guard, so before `ctx.withdraw`: reject a zero amount, check the recipient, load the caller's balance under `ctx.caller()` (never an account taken from the arguments), compute the remainder with `checked_sub` and return your error on `None`, then save the remainder or remove the leaf at zero.

The method below does exactly this against `CREDITS`.

<!-- guide-test: module -->
```rust
    /// Debits the caller's total of the asset and withdraws the amount.
    #[method]
    pub fn withdraw(
        ctx: &mut Ctx,
        asset: AssetId,
        amount: U256,
        to: L1Address,
    ) -> Result<(), Error> {
        if amount.is_zero() {
            return Err(Error::ZeroAmount);
        }
        if !to.is_user() {
            return Err(Error::InvalidRecipient);
        }
        let key = (ctx.caller(), asset);
        let total = CREDITS.may_load(ctx, &key).unwrap_or(U256::ZERO);
        let Some(remaining) = total.checked_sub(amount) else {
            return Err(Error::InsufficientCredit);
        };
        if remaining.is_zero() {
            CREDITS.remove(ctx, &key);
        } else {
            CREDITS.save(ctx, &key, &remaining);
        }
        ctx.withdraw(to, asset, amount);
        Ok(())
    }
```

## Build

`cargo zeno build` compiles for `wasm32v1-none` with the link flags set, strips the module, validates it and writes `artifacts/<name>.wasm`, `<name>.interface.json` and `<name>.hashes.json`. Use it instead of `cargo build`. Commit the artifacts.

`[package.metadata.zeno]` in `Cargo.toml` lists the artifacts of the crate:

```toml
[package.metadata.zeno]
artifacts = [{ name = "demo" }, { name = "demo-test", features = ["test-hook"] }]
```

- `name`: the artifact name. It is required and unique in the workspace.
- `features`: the Cargo features of the build. A feature set gives one crate several artifacts, as `vault` and `vault-testhook` show in `examples/vault`. Each feature must exist in the crate's `[features]` table, for example `test-hook = []`, as `examples/vault/Cargo.toml` declares.
- `metadata`: `"immutable"` or `"owner:<address>"`. `cargo zeno build` writes the matching deploy metadata to the hashes file.

Never deploy a test-only artifact. A hook still runs on real deposits if someone deploys it, so it must keep the ledger's invariants: credit no more than it claims, and never write a balance directly.

## Testing

`cargo zeno test` rebuilds the artifacts and runs the crate's tests against `zeno-exec`. `docs/testing.md` lists the harness. The test below deploys the crate's own artifact, buys storage, sends a deposit and checks the effects.

<!-- guide-test: tests -->
```rust
#[test]
fn credit_claims_the_deposit() {
    let mut d = TestDomain::new();
    let a = d.account(1);
    let c = d.deploy(env!("CARGO_PKG_NAME"), &a);
    d.buy_storage(&c, &a, 4096).assert_success();

    let asset = AssetId::from_zts([1; 10]);
    let r = d.call_with_deposit(&c, &a, "credit", (), asset, U256::from(100u64));
    r.assert_success();
    assert_eq!(r.claimed_deposit, U256::from(100u64));
    assert!(r.outbox.is_empty());
    assert_eq!(r.events[0].topics[0], sha3_256(&[b"Credited"]));
    let key = sha3_256(&[&[0x02], &a.caller().to_bytes(), &asset.to_bytes()]);
    assert_eq!(
        d.storage(&c, &key),
        Some(U256::from(100u64).to_be_bytes().to_vec())
    );
}
```

A withdrawal above the balance must fail and register nothing.

<!-- guide-test: tests -->
```rust
#[test]
fn withdraw_above_the_balance_fails() {
    let mut d = TestDomain::new();
    let a = d.account(1);
    let c = d.deploy(env!("CARGO_PKG_NAME"), &a);
    d.buy_storage(&c, &a, 4096).assert_success();

    let asset = AssetId::from_zts([1; 10]);
    d.call_with_deposit(&c, &a, "credit", (), asset, U256::from(100u64))
        .assert_success();
    let r = d.call(&c, &a, "withdraw", (asset, U256::from(150u64), a.address()));
    r.assert_error(&Error::InsufficientCredit);
    assert!(r.outbox.is_empty());
}
```

A valid withdrawal debits the ledger and registers one withdrawal. A zero amount or a non-user recipient fails and registers nothing.

<!-- guide-test: tests -->
```rust
#[test]
fn withdraw_debits_the_ledger_and_checks_its_arguments() {
    let mut d = TestDomain::new();
    let a = d.account(1);
    let c = d.deploy(env!("CARGO_PKG_NAME"), &a);
    d.buy_storage(&c, &a, 4096).assert_success();

    let asset = AssetId::from_zts([1; 10]);
    let key = sha3_256(&[&[0x02], &a.caller().to_bytes(), &asset.to_bytes()]);
    d.call_with_deposit(&c, &a, "credit", (), asset, U256::from(100u64))
        .assert_success();

    let r = d.call(&c, &a, "withdraw", (asset, U256::from(40u64), a.address()));
    r.assert_success();
    assert_eq!(
        r.outbox,
        [Outbox {
            source_contract: c.id().to_bytes(),
            recipient: a.address(),
            asset,
            amount: U256::from(40u64),
        }]
    );
    assert_eq!(
        d.storage(&c, &key),
        Some(U256::from(60u64).to_be_bytes().to_vec())
    );

    let r = d.call(&c, &a, "withdraw", (asset, U256::ZERO, a.address()));
    r.assert_error(&Error::ZeroAmount);
    assert!(r.outbox.is_empty());
    let nobody = L1Address::new([0; 20]);
    let r = d.call(&c, &a, "withdraw", (asset, U256::from(1u64), nobody));
    r.assert_error(&Error::InvalidRecipient);
    assert!(r.outbox.is_empty());

    let r = d.call(&c, &a, "withdraw", (asset, U256::from(60u64), a.address()));
    r.assert_success();
    assert_eq!(d.storage(&c, &key), None);
}
```

## Deploy

1. Run `cargo zeno build` and keep `artifacts/<name>.wasm` and the `metadata` from `<name>.hashes.json`.
2. Deploy the module through zeno-console. Choose `IMMUTABLE` for a contract that never changes, or an owner address for a contract the owner can upgrade.
3. Never deploy a test-only artifact such as `vault-testhook`.
4. Buy storage for the contract before the first write. A call that writes without bought storage fails with `STORAGE_QUOTA_EXCEEDED` and the runtime refunds its deposit.
