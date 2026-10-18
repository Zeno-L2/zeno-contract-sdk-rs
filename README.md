# zeno-contract-sdk-rs

Rust SDK for Zeno WASM contracts.

- `crates/zeno-contract`: the `no_std` runtime (frame reader, codec, typed storage, effects encoder).
- `crates/zeno-contract-macros`: the `#[contract]` attribute macros.
- `examples/counter`: a counter contract (`v2` feature adds `decrement`).
- `examples/vault`: the reference vault (deposits credited in full, withdrawals to L1 user addresses).
- `tests/vault-shape`: the vault declarations in macro form.
- `crates/zeno-testing`: the test harness over `zeno-exec run`.
- `crates/cargo-zeno`: `cargo zeno new`, `build`, `test` and `explain`.
- `templates/contract`: the `cargo zeno new` scaffold.

## Docs

[`AGENTS.md`](AGENTS.md) for agents, then [`docs/guide.md`](docs/guide.md), [`docs/api.md`](docs/api.md), [`docs/testing.md`](docs/testing.md) and [`docs/errors.md`](docs/errors.md).

## Commands

- `cargo zeno new <name> [--sdk-path <path>]`: scaffolds a contract project.
- `cargo zeno build [--check] [--json]`: builds, strips and validates every artifact listed in `[package.metadata.zeno]` and writes it to the crate's `artifacts/`. `--check` rebuilds in a scratch copy and fails when a committed artifact differs.
- `cargo zeno test [--json]`: builds the artifacts, then runs each contract crate's tests against `zeno-exec`.
- `cargo zeno explain <code>`: prints the cause and fix of an error code from `docs/errors.md`.

`cargo zeno` finds `zeno-exec` in this order: the `ZENO_EXEC` variable, the cache at `~/.cache/zeno/zeno-exec/<version>`, the pinned download once a release is pinned, and a build of `../zeno-executor` into `target/zeno/zeno-exec`.

Outside this repository, install the tool with `cargo install --git https://github.com/Zeno-L2/zeno-contract-sdk-rs --tag <tag> --locked cargo-zeno`.

## Reference vault

The reference vault is production-quality example code for custody-holding contracts: build DEXes, escrows and bridge pools on it. It is also the testnet demo contract for deposits and withdrawals. It has no published mainnet contract id, and no mainnet deployment is planned.

`examples/vault/artifacts/vault.*` is the reference build; `vault-testhook.*` is test-only and adds `claim_partial`.

For the testnet demo, deploy `vault.wasm` with the 23-byte `metadata` from `vault.hashes.json` (`IMMUTABLE`, zero owner); `SHA3-256(vault.wasm)` equals its `code_hash`.

The vault writes a balance leaf per (caller, asset), so buy storage before the first deposit; a deposit with no quota is refunded in full. A withdrawal recipient must be a user address (first byte `0x00`, not all zero).

Every new (caller, asset) pair adds a leaf that is charged to the vault's one shared storage quota, so a flood of tiny first deposits from new pairs can use the quota up. Funds are never lost: a refused first deposit is refunded in full, deposits to existing balances and every withdrawal need no quota, and anyone can restore service by buying storage. The operator monitors the quota and tops it up.

Only `vault.*` is ever deployed. Never deploy a `vault-testhook.*` artifact.

`vault.wasm` claims every deposit in full; a testnet partial-claim refund flow needs another contract.

## Tests

`cargo test --locked --workspace --exclude counter --exclude vault` runs the host tests. `cargo zeno test` runs the contract crates' tests through `zeno-exec`. The layout, session, `check_mode` and `guide_snippets` tests need built wasm modules or `zeno-exec` and check nothing without them, so they pass silently on a plain run. `guide_snippets` also needs the crates.io index. Set `ZENO_REQUIRE_WASM_TESTS=1` to make them fail when their variables are unset or empty. The full run:

```bash
WS=$(pwd -P)
cargo zeno build
ZENO_REQUIRE_WASM_TESTS=1 \
ZENO_EXEC="${ZENO_EXEC:-$WS/target/zeno/zeno-exec}" ZENO_SESSION_DIR="$WS/target/zeno" \
ZENO_LAYOUT_WASM="$WS/target/zeno/counter.wasm $WS/target/zeno/counter-v2.wasm $WS/target/zeno/vault-shape.wasm $WS/target/zeno/vault.wasm $WS/target/zeno/vault-testhook.wasm" \
  cargo test --locked --workspace --exclude counter --exclude vault
cargo zeno test
```

Never export `RUSTFLAGS`: `cargo zeno build` sets the link flags for the wasm build only, and exported flags break host test builds.
