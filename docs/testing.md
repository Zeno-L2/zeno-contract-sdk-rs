# Testing

`cargo zeno test` rebuilds the artifacts, then runs each contract crate's tests (`cargo test --lib`) against `zeno-exec`. It sets `ZENO_FRESH=1`, which proves to the harness that the artifacts are current. A contract test run any other way, such as a plain `cargo test`, panics `Z0402`. Run host-only crates with `cargo test --workspace --exclude <contract crates>`.

Tests use `zeno-testing`, which drives one `zeno-exec run` session per `TestDomain`. Add it as a dev-dependency, and start the test file with `extern crate std;` and `use zeno_testing::*;`.

## TestDomain

| Method | Behaviour |
|---|---|
| `TestDomain::new()` | starts a session |
| `account(n)` | the account with address `0x00` followed by 19 bytes of `n` |
| `deploy(artifact, &from)` | deploys the artifact by name with `from` as owner |
| `deploy_immutable(artifact, &from)` | deploys the artifact as immutable |
| `contract_at(id)` | a handle for a contract id; `call` works only on ids that `deploy` produced |
| `call(&c, &from, method, args)` | calls a method; `args` is a tuple, `()` for none |
| `call_with_deposit(&c, &from, method, args, asset, amount)` | calls a method with a deposit attached |
| `call_raw(&c, &from, payload, deposit)` | sends raw payload bytes with an optional deposit |
| `deposit(&c, &from, asset, amount)` | sends a deposit with an empty payload |
| `upgrade(&c, &from, artifact)` | upgrades an owned contract |
| `transfer_ownership(&c, &from, &to)` | moves ownership |
| `buy_storage(&c, &from, bytes)` | buys storage quota |
| `set_storage(&c, local_key, value)` | writes a local key directly; `None` deletes it |
| `storage(&c, local_key)` | reads a local key |

Buy storage before the first call that writes. Without it the call fails with `STORAGE_QUOTA_EXCEEDED` and refunds the deposit.

## Receipt

A call returns a `Receipt` with `status`, `gas_used`, `abort_code`, `return_data`, `claimed_deposit`, `events` (`topics`, `data`), `outbox` (`source_contract`, `recipient`, `asset`, `amount`) and `state_diff` (`path`, `value`).

- `assert_success()`: the status is `Success`.
- `assert_error(&Error::Variant { .. })`: a revert with the variant's code and encoded fields, and no state writes or events.
- `assert_abort(code)`: a revert with the code, empty return data, and no state writes or events. The `codes` module holds the SDK codes.
- `decode_return::<T>()`: decodes the return data as `T`.

## Keys and leaf paths

`storage` and `set_storage` take the contract-local key, as the interface file defines it. For an unhashed entry that is `[namespace]` plus the key bytes. For a hashed entry it is `sha3_256(&[&[namespace], key bytes...])`:

```rust
let key = sha3_256(&[&[0x01], &account.caller().to_bytes(), &asset.to_bytes()]);
```

`Contract::leaf_path(&key)` gives the state path that appears in a receipt's `state_diff`. A test of the writes of one call compares `path` against `c.leaf_path(&key)`; the storage quota leaf is at `c.leaf_path(&[0x00, 0x05])`.

## Seeding and malformed input

- `set_storage` seeds a state that the contract's own methods cannot reach, such as a full balance or a stored value of the wrong width.
- `call_raw` sends payloads that `call` cannot encode: a truncated argument, an unknown selector, or an empty payload with a deposit.
- `contract_at` makes a handle for an id that holds no contract, to test the runtime's refund.

## A test-only artifact

A feature set in `[package.metadata.zeno]` builds an artifact with extra methods for tests. Tests deploy it by its artifact name. The reference vault builds `vault-testhook` from the `test-hook` feature, which adds `claim_partial`. The tests must compile with and without the feature, because the interface step compiles them with it and `cargo zeno test` compiles them without it.

Never deploy a test-only artifact. A hook still runs on real deposits if someone deploys it, so it must keep the ledger's invariants: credit no more than it claims, and never write a balance directly.

## Example

[`examples/vault/src/tests.rs`](../examples/vault/src/tests.rs) is the full example. It covers deposits, withdrawals, rejected input, overflow, a missing storage quota and the test-only artifact for the reference vault.
