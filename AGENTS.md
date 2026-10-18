# AGENTS.md

Guidance for contract authors and agents working in this repository.

## 1. What this repository is

A Rust SDK for Zeno WASM contracts: `no_std` modules with a three-import ABI, built by `cargo zeno`, tested against zeno-executor's runtime through `zeno-exec`.

- `crates/`: `zeno-contract` (runtime), `zeno-contract-macros` (`#[contract]` and friends), `zeno-testing` (test harness), `cargo-zeno` (the tool).
- `examples/`: `counter` (smallest contract) and `vault` (the reference vault).
- `templates/contract`: the `cargo zeno new` scaffold.
- `docs/`: `guide.md`, `api.md`, `testing.md`, `errors.md`.

`examples/vault` is the reference vault: the pattern to copy for custody-holding contracts (DEXes, escrows, bridge pools). It is also the testnet demo contract. It has no published mainnet contract id, and no mainnet deployment is planned.

## 2. The model

- A module has one export, `run(ptr, len) -> i64`, and exactly three imports: `env.storage_len`, `env.storage_read`, `env.abort`.
- A call performs no writes. The module returns its writes, events and withdrawals as effects, and the runtime applies them when the call succeeds.
- `storage_read` sees the state from before the call. The SDK keeps an overlay, so a read sees the call's earlier writes.
- Value arrives as a deposit and leaves as `ctx.withdraw`. There are no protocol balances and no transfers: the runtime does not limit `ctx.withdraw` to what the contract received, so the contract's ledger, debited with `checked_sub` before the withdrawal, is the only guard.
- There is no view entry point, so every method call is an input. Clients read state through the gateway `storage` view with the keys in the interface file.

## 3. Rules that make a module deployable

1. `#![no_std]`, no `alloc`, built only by `cargo zeno build` (it sets the target, link flags and strip step). A plain host `cargo build` does not produce a deployable module.
2. No `f32` or `f64` anywhere, dependencies included: `Z0301`.
3. No `extern` block and no dependency that adds an import: `Z0302`.
4. No post-MVP instruction. The `wasm32v1-none` target guarantees this when you never bypass the build: `Z0303`.
5. The module and its instrumented form stay within the size limit: `Z0304`.

## 4. Commands

- `cargo zeno new <name> [--sdk-path <path>]`: scaffolds a contract crate.
- `cargo zeno build [--check] [--json]`: builds, strips and validates every artifact and writes it to `artifacts/`. `--check` fails when a committed artifact differs.
- `cargo zeno test [--json]`: builds, then runs the contract crates' tests against `zeno-exec`.
- `cargo zeno explain <code>`: prints the cause and fix of an error code.

One contract per crate. The `#[contract]` module sits in `src/lib.rs`, because the interface step needs it (`Z0209`). Several artifacts of one crate come from feature sets in `[package.metadata.zeno]`, as the reference vault's `vault-testhook` shows.

## 5. API

| Item | Use |
|---|---|
| `#[contract]` | on an inline `mod`; generates `run`, dispatch and the interface |
| `#[method]`, `#[method(payable)]`, `#[method(returns(a, b))]` | a callable function; `payable` may claim the deposit; `returns` names the outputs |
| `#[on_deposit]` | the handler for an empty payload; one per module |
| `#[contract_error]` | the module's error enum; codes from 1, a `///` line per variant |
| `#[event]`, `#[indexed]` | an event struct; up to 3 indexed fields |
| `#[storage(0xNN)]`, `#[storage(0xNN, key(a, b))]` | a `const` `Item<V>` or `Map<K, V>` at a namespace |
| `ctx.caller()`, `contract_id()`, `chain_id()`, `code_hash()`, `momentum()`, `input_index()` | call context |
| `ctx.deposit()` | the attached deposit, `None` when zero |
| `ctx.claim_deposit(n)` | claims `n` of the deposit; above the claimable amount aborts -5 |
| `ctx.emit(event)` | records an event |
| `ctx.withdraw(to, asset, n)` | registers a withdrawal to a user address; no balance bounds it, so debit the caller's ledger first |
| `Item`, `Map` | `may_load`, `save`, `remove` |
| Types (interface type) | integers and `bool` (same name), `U256` (`u256`), `Hash32`/`Caller`/`ContractId`/`CodeHash` (`hash`), `AssetId` (`bytes32`), `L1Address` (`l1Address`), `[u8; N]` with N 1 to 32 (`bytesN`); arguments also take `&[u8]` (`bytes`) and `&str` (`string`) |

`docs/api.md` has the full tables.

`ctx.withdraw` is unbounded: nothing compares a withdrawal with what the contract received. Before it, reject a zero amount, check the recipient, debit the caller's ledger keyed by `ctx.caller()` with `checked_sub`, and remove the leaf at zero:

```rust
if amount.is_zero() {
    return Err(Error::ZeroAmount);
}
if !to.is_user() {
    return Err(Error::InvalidRecipient);
}
let key = (ctx.caller(), asset);
let balance = BALANCES.may_load(ctx, &key).unwrap_or(U256::ZERO);
let Some(rest) = balance.checked_sub(amount) else {
    return Err(Error::InsufficientBalance);
};
if rest.is_zero() {
    BALANCES.remove(ctx, &key);
} else {
    BALANCES.save(ctx, &key, &rest);
}
ctx.withdraw(to, asset, amount);
```

## 6. Abort codes

| Code | Name | Meaning |
|---|---|---|
| -1 | `MALFORMED_PAYLOAD` | the frame, selector or arguments do not decode |
| -2 | `UNKNOWN_SELECTOR` | no method has the selector |
| -3 | `NO_DEPOSIT_HANDLER` | an empty payload and no `#[on_deposit]` |
| -4 | `CAP_EXCEEDED` | an effect breaks a cap or does not fit a buffer |
| -5 | `CLAIM_ABOVE_DEPOSIT` | `claim_deposit` above the claimable amount |
| -6 | `STORED_VALUE_DECODE` | a stored value fails to decode |
| -7 | `ALLOCATION_FAILURE` | reserved, never raised |
| -8 | `PANIC` | the module panicked |
| -9 | `INVALID_RECIPIENT` | `withdraw` to a non-user address |

Contract codes are positive. The variant's fields are the revert data.

## 7. Storage rules

- Namespaces run from `0x01` to `0xff`, one per entry, distinct in a module.
- Key fields are fixed-width. `Key` exists only for fixed-width types and tuples of them.
- A key of more than 30 bytes in total is hashed: the local key is `SHA3-256(ns || key)`. Otherwise it is `ns || key`.
- Buy storage before the first write, else the call fails with `STORAGE_QUOTA_EXCEEDED`.
- An absent leaf reads `None`.

## 8. Testing

- Run `cargo zeno test`. A plain `cargo test` of a contract crate panics `Z0402`.
- Tests deploy by artifact name: `d.deploy("name", &account)`.
- Call `buy_storage` before the first call that writes.
- Deploy only production artifacts, such as `vault.*`. Never deploy a test-only artifact such as `vault-testhook.*`.
- `docs/testing.md` lists the harness.

## 9. Fixing an error

1. Read `error[Zxxxx]` and its `fix:` line.
2. Run `cargo zeno explain Zxxxx`.
3. Apply the fix and rerun.

With `--json`, read `errors[].code` and `errors[].fix`.

## 10. Working on this repository

Run these before every commit, from the repository root:

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --exclude counter --exclude vault
cargo zeno build --check
cargo zeno test
```

Then run the full run, which makes the environment-gated tests run instead of returning early:

```bash
WS=$(pwd -P)
cargo zeno build
ZENO_REQUIRE_WASM_TESTS=1 \
ZENO_EXEC="${ZENO_EXEC:-$HOME/.cache/zeno/zeno-exec/$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/cargo-zeno/zeno-exec.pin)/zeno-exec}" \
ZENO_SESSION_DIR="$WS/target/zeno" \
ZENO_LAYOUT_WASM="$WS/target/zeno/counter.wasm $WS/target/zeno/counter-v2.wasm $WS/target/zeno/vault-shape.wasm $WS/target/zeno/vault.wasm $WS/target/zeno/vault-testhook.wasm" \
  cargo test --locked --workspace --exclude counter --exclude vault
cargo zeno test
```

- `cargo zeno build` regenerates `artifacts/`. Commit them. Never hand-edit an artifact.
- Never export `RUSTFLAGS`: it breaks host test builds.
- A new contract crate joins the `--exclude` lists in `README.md` and `.github/workflows/ci.yml`.

## 11. Links

- [Guide](docs/guide.md)
- [API reference](docs/api.md)
- [Testing](docs/testing.md)
- [Error codes](docs/errors.md)
- [Reference vault](examples/vault)
