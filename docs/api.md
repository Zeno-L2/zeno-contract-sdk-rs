# API reference

Everything a contract imports comes from `use zeno_contract::prelude::*;`. The prelude holds the attributes `contract`, `contract_error`, `event`, `method`, `on_deposit` and `storage`, and the types `AssetId`, `Caller`, `CodeHash`, `ContractId`, `Ctx`, `Deposit`, `Hash32`, `Item`, `L1Address`, `Map`, `Momentum` and `U256`.

## Attributes

| Attribute | Applies to | Rules |
|---|---|---|
| `#[contract]` | an inline `mod name { use super::*; ... }` | takes no arguments (`Z0102`); every other attribute sits inside it (`Z0101`) |
| `#[method]` | a function in the module | the first parameter is `ctx: &Ctx` or `ctx: &mut Ctx`, then named arguments (`Z0103`); the selector is the first 4 bytes of `SHA3-256(name)` and is unique (`Z0108`); returns `()`, one fixed-width type, a tuple of 2 to 4, or a `Result` of those with the module's error enum (`Z0105`, `Z0122`) |
| `#[method(payable)]` | same | takes `&mut Ctx` (`Z0104`); may claim the deposit |
| `#[method(returns(a, b))]` | same | names each output; the count equals the outputs (`Z0106`) |
| `#[on_deposit]` | a function | one per module, takes only `ctx`, returns `()` or a `Result` with the error enum (`Z0121`) |
| `#[contract_error]` | an enum | one per module, unit or named-field variants (`Z0112`); every variant has `= n` with `n >= 1` (`Z0109`), a unique code (`Z0110`) and a `///` line (`Z0111`) |
| `#[event]` | a named-field struct | unique name, no generics (`Z0115`); the topic is `SHA3-256(name)` |
| `#[indexed]` | an event field | at most 3 per event (`Z0113`); the type fits a 32-byte topic (`Z0114`) |
| `#[storage(0xNN)]` | a `const NAME: Item<V> = Item::new();` | namespace is a literal from `0x01` to `0xff` (`Z0116`) and unique (`Z0117`); the type is `Item<V>` or `Map<K, V>` (`Z0120`) |
| `#[storage(0xNN, key(a, b))]` | a `const NAME: Map<K, V> = Map::new();` | one name per key field (`Z0118`) |

Arguments the attributes do not accept give `Z0107`. A type the codec does not support gives `Z0122`. A storage handle used outside a `#[contract]` module gives `Z0119`.

## Ctx

| Method | Returns | Behaviour |
|---|---|---|
| `caller()` | `Caller` | the account that sent the call |
| `contract_id()` | `ContractId` | this contract's id |
| `chain_id()` | `Hash32` | the chain id |
| `code_hash()` | `CodeHash` | the hash of this contract's code |
| `momentum()` | `Momentum` | `height`, `time` and `hash` of the momentum the call executes in |
| `input_index()` | `u64` | the index of the input that carries the call |
| `deposit()` | `Option<Deposit>` | the attached deposit, `None` when its amount is zero |
| `claim_deposit(amount)` | `()` | claims `amount`; a second claim replaces the first; above the claimable amount aborts -5 |
| `emit(event)` | `()` | records an event; aborts -4 on a cap breach |
| `withdraw(to, asset, amount)` | `()` | registers a withdrawal; aborts -9 unless `to` is a user address, then -4 on a cap breach; no balance bounds it |

The claimable amount is the deposit in a deposit handler or a `payable` method, and zero elsewhere. The runtime refunds the part of a deposit that the call does not claim.

## Item and Map

| Method | Behaviour |
|---|---|
| `Item::may_load(ctx)`, `Map::may_load(ctx, &key)` | the stored value, or `None` when absent; aborts -6 when the stored bytes do not decode |
| `Item::save(ctx, &v)`, `Map::save(ctx, &key, &v)` | stores the value; a later save replaces it; aborts -4 when the effects arena is full |
| `Item::remove(ctx)`, `Map::remove(ctx, &key)` | removes the value |

A read sees the call's earlier writes. A map key is a fixed-width type or a tuple of 2 to 4 of them.

## Types

| Rust type | Interface type | Width | Argument | Storage value | Key field | Indexed field |
|---|---|---|---|---|---|---|
| `u8`, `u16`, `u32`, `u64`, `u128` | same name | 1, 2, 4, 8, 16 | yes | yes | yes | no |
| `i8`, `i16`, `i32`, `i64`, `i128` | same name | 1, 2, 4, 8, 16 | yes | yes | yes | no |
| `bool` | `bool` | 1 | yes | yes | yes | no |
| `U256` | `u256` | 32 | yes | yes | yes | yes |
| `[u8; N]`, N from 1 to 32 | `bytesN` | N | yes | yes | yes | only N = 32 |
| `Hash32`, `Caller`, `ContractId`, `CodeHash` | `hash` | 32 | yes | yes | yes | yes |
| `AssetId` | `bytes32` | 32 | yes | yes | yes | yes |
| `L1Address` | `l1Address` | 20 | yes | yes | yes | no |
| `&[u8]` | `bytes` | `u32` length, then bytes | yes | no | no | no |
| `&str` | `string` | `u32` length, then UTF-8 bytes | yes | no | no | no |

All integers are big-endian. Key fields are fixed-width only; a key of more than 30 bytes in total is hashed.

An asset id with a non-zero byte in `[0:22]` aborts -1 as an argument and -6 from storage. `AssetId::from_zts` builds an id from a 10-byte ZTS.

## Abort codes

| Code | Name | Raised when |
|---|---|---|
| -1 | `MALFORMED_PAYLOAD` | the frame, selector or arguments do not decode, or the payload is above 8 KiB |
| -2 | `UNKNOWN_SELECTOR` | no method has the payload's selector |
| -3 | `NO_DEPOSIT_HANDLER` | an empty payload reaches a module with no `#[on_deposit]` |
| -4 | `CAP_EXCEEDED` | an effect breaks a cap or does not fit a buffer |
| -5 | `CLAIM_ABOVE_DEPOSIT` | `claim_deposit` asks for more than the claimable amount |
| -6 | `STORED_VALUE_DECODE` | a stored value fails to decode |
| -7 | `ALLOCATION_FAILURE` | reserved, never raised |
| -8 | `PANIC` | the module panicked |
| -9 | `INVALID_RECIPIENT` | `withdraw` to an address that is not a user address |

A user address starts with byte `0x00` and is not all zero. Contract codes are positive and carry the variant's fields as revert data.

## Interface file

`cargo zeno build` writes `artifacts/<name>.interface.json`. Its members are `interface_version`, `encoding`, `code_hash`, `methods`, `on_deposit`, `events`, `errors` and `storage`.

- A method has `name`, `selector`, `payable`, `readonly`, `inputs` and `outputs`.
- `on_deposit` is the handler (`name`, `outputs`) or `null`.
- An event has `name`, `topic` (`SHA3-256(name)`) and `fields`, each field with an `indexed` flag.
- An error has `code`, `name`, `fields` and `description`. The list holds the contract's codes, then the SDK codes the module can raise.
- A storage entry has `name`, `kind` (`item` or `map`), `namespace`, `key`, `hashed` and `value`. The local key is `namespace || key fields` when `hashed` is false and `SHA3-256` of that when it is true.

Clients follow the `hashed` flag and read leaves through the gateway `storage` view. [`examples/vault/artifacts/vault.interface.json`](../examples/vault/artifacts/vault.interface.json) is an instance: the reference vault's balances are a hashed map.

## Limits

| Limit | Value | On breach |
|---|---|---|
| Payload | 8 KiB | -1 |
| Effects arena (writes, events, withdrawals) | 8 KiB | -4 |
| Encoded output buffer | 12 KiB | -4 |
| Events per call | 64 | -4 |
| Topics per event | 4 (the name hash and 3 indexed fields) | -4 |
| Data per event | 1 KiB | -4 |
| Withdrawals per call | 32 | -4 |
| Return data or revert data | 4 KiB | -4 |

The 8 KiB arena keeps one input's effects below the runtime's 16 KiB value cap. The runtime refuses a call payload above 8 KiB before the call runs; the -1 is the SDK's own guard.
