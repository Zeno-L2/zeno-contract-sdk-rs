# Error codes

`cargo zeno explain <code>` prints one section of this file.

## Z0101 attribute outside a contract module

Cause: a contract attribute sits on an item outside a `#[contract]` module.
Fix: move the item into the module.

## Z0102 malformed contract attribute

Cause: `#[contract]` is applied to something other than an inline module, or it has arguments.
Fix: write `#[contract] mod name { use super::*; ... }`.

## Z0103 invalid method signature or reserved name

Cause: a contract function is async, const, unsafe, extern, generic, takes `self`, has a parameter that is not a plain name, does not start with a `Ctx` parameter, or uses a name reserved for generated code.
Fix: take `ctx: &Ctx` or `ctx: &mut Ctx` first, then named arguments; `run` and `__zeno*` are reserved for generated code.

## Z0104 payable method with a read-only context

Cause: a `payable` method takes `&Ctx`.
Fix: take `&mut Ctx`, or drop `payable`.

## Z0105 invalid method result type

Cause: a method returns a `Result` without two type arguments, or whose error type is not the module's `#[contract_error]` enum.
Fix: return the module's error enum.

## Z0106 returns names do not match the outputs

Cause: `returns(...)` names a different count than the return type has outputs.
Fix: name each output: #[method(returns(a, b))].

## Z0107 unknown or malformed attribute argument

Cause: an attribute has an argument it does not accept, or one contract item carries a second contract attribute.
Fix: use `payable` or `returns(name, ...)` on `#[method]`, `#[storage(0xNN)]` or `#[storage(0xNN, key(a, b))]` on a storage constant, no arguments on `#[on_deposit]` and `#[event]`, and one contract attribute per item.

## Z0108 duplicate selector

Cause: two methods have the same 4-byte selector.
Fix: rename one method.

## Z0109 invalid error code

Cause: an error variant has no `= n` code, or the code is not an integer from 1 to 2147483647.
Fix: give every variant `= n` with `n >= 1`.

## Z0110 duplicate error code

Cause: two error variants have the same code.
Fix: give each variant its own code.

## Z0111 error variant without a description

Cause: an error variant has no `///` line.
Fix: add a `///` line; it becomes the interface description.

## Z0112 invalid error enum

Cause: the module has a second `#[contract_error]` enum, the attribute is not on an enum, or the enum has generics or tuple variants.
Fix: use unit or named-field variants in one enum.

## Z0113 too many indexed event fields

Cause: an event indexes more than 3 fields.
Fix: index at most 3 fields.

## Z0114 field cannot be indexed

Cause: an indexed event field has a type that does not fit in a topic.
Fix: index only Hash32, Caller, ContractId, CodeHash, AssetId, [u8; 32] or U256, or drop #[indexed].

## Z0115 invalid event

Cause: an event is not a named-field struct, has generics, or shares its name with another event.
Fix: use a named-field struct with a unique name.

## Z0116 invalid storage namespace

Cause: `#[storage]` has no namespace, or the namespace is not an integer literal in 0x01..=0xff.
Fix: use a literal from `0x01` to `0xff`.

## Z0117 duplicate storage namespace

Cause: two storage entries share a namespace.
Fix: give each entry its own namespace.

## Z0118 key names do not match the key

Cause: `key(...)` is on an `Item`, a `Map` has no `key(...)`, or `key(...)` names a different count than the key has fields.
Fix: name one key field per tuple element.

## Z0119 storage handle outside a contract module

Cause: an `Item` or `Map` constant has no `#[storage]` attribute, or `Item::new()` or `Map::new()` runs outside a `#[contract]` module.
Fix: add `#[storage(0xNN)]` inside a `#[contract]` module.

## Z0120 invalid storage declaration

Cause: `#[storage]` is not on a `const`, or the constant's type is not `Item<V>` or `Map<K, V>` with the right number of arguments.
Fix: declare `const NAME: Item<V> = Item::new();`.

## Z0121 invalid deposit handler

Cause: the module has a second `#[on_deposit]` function, or the handler takes arguments after the ctx or returns more than `()`.
Fix: keep one handler taking only `ctx`.

## Z0122 unsupported type

Cause: a method argument, storage value, key or return value has a type the codec does not support.
Fix: use u8..u128, i8..i128, bool, U256, [u8; N] with N in 1..=32, Hash32, Caller, ContractId, CodeHash, AssetId, L1Address, &[u8] or &str; keys and return values take fixed-width types only.

## Z0201 zeno-exec not found

Cause: every way of finding `zeno-exec` failed or was skipped: the `ZENO_EXEC` variable, the cache, the pinned download and the sibling checkout.
Fix: set `ZENO_EXEC` to a zeno-exec binary, or publish and pin a release (https://github.com/Zeno-L2/zeno-executor/releases/download/zeno-exec-v<version>/zeno-exec-<platform>), or check out zeno-executor beside the workspace (../zeno-executor).

## Z0202 zeno-exec checksum mismatch

Cause: a cached or downloaded `zeno-exec` does not match the pinned SHA-256.
Fix: the file was deleted; run the command again, and report a mismatch that persists.

## Z0203 release profile differs from the required one

Cause: the workspace `[profile.release]` lacks a required key, sets it to another value, or sets an extra key.
Fix: set `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true` and `debug = false` in `[profile.release]` of the workspace Cargo.toml, and remove any other key.

## Z0204 Cargo.lock missing

Cause: the workspace root has no `Cargo.lock`, so the build cannot be locked.
Fix: run `cargo generate-lockfile` and commit the file.

## Z0205 zeno-exec revision differs from the pin

Cause: the `zeno-exec` in use was built from another zeno-executor revision than the one this tool pins. This is a warning, and an error under `build --check`.
Fix: build zeno-executor at the pinned revision, or set `ZENO_EXEC` to a binary of it.

## Z0206 zeno-exec built from a modified tree

Cause: the `zeno-exec` in use was built from a working tree with local changes. This is a warning.
Fix: build from a clean checkout; untracked files such as `.DS_Store` count.

## Z0207 artifact file missing or different

Cause: `build --check` rebuilt the artifacts and a committed file is missing or differs.
Fix: run `cargo zeno build` and commit, or find the non-reproducible input (check for `[profile.release]` in `.cargo/config.toml`).

## Z0208 no contract crate or malformed contract metadata

Cause: no workspace member has a `[package.metadata.zeno]` table, a contract crate has no `cdylib` target, or the table is malformed.
Fix: write `[package.metadata.zeno]` with `artifacts = [{ name = "a-b", features = ["f"], metadata = "immutable" }]`; only `artifacts` is a key, `name` is required and unique, and `metadata` is `immutable` or `owner:<address>`.

## Z0209 tool step failed

Cause: a cargo, go, curl or zeno-exec command failed, or a module or file could not be read or written.
Fix: read the output above; the message names the command.

## Z0210 usage error

Cause: the command, a flag or an argument is not one `cargo zeno` accepts.
Fix: run `cargo zeno --help` for the usage text.

## Z0211 cannot create the project

Cause: `cargo zeno new` got a name that is not usable, a directory that already exists, or an `--sdk-path` that is not a zeno-contract-sdk-rs checkout.
Fix: use a name matching `[a-z][a-z0-9_]*` that is not a Rust keyword, `test`, `tests`, `host`, `zeno_contract` or `zeno_testing`; choose a directory that does not exist; point `--sdk-path` at a zeno-contract-sdk-rs checkout.

## Z0212 unknown error code

Cause: `cargo zeno explain` got a code that has no section in this file.
Fix: this file lists every code.

## Z0213 contract tests failed

Cause: a contract crate's tests failed or did not run.
Fix: the failing tests are listed above.

## Z0301 float type or instruction

Cause: the module uses f32 or f64, which the runtime refuses.
Fix: remove f32/f64 use or the dependency that adds it (or disable its float feature); inspect dependencies with `cargo tree`.

## Z0302 disallowed import

Cause: the module imports a function other than `env.storage_len`, `env.storage_read` and `env.abort`, or one with another signature.
Fix: remove the dependency that adds it; inspect with `cargo tree`.

## Z0303 post-MVP instruction or type

Cause: the module uses a WebAssembly feature newer than the MVP set, such as sign extension, bulk memory, SIMD, reference types, atomics or multi-value results.
Fix: build only with `cargo zeno build` (target `wasm32v1-none`); remove the dependency that emits it.

## Z0304 module too large

Cause: the module or its instrumented form exceeds the size limit.
Fix: reduce code size: fewer dependencies, smaller methods.

## Z0305 module did not compile

Cause: the engine refused to compile the module, or the check itself panicked.
Fix: report it with the module.

## Z0399 module refused

Cause: the deploy checks refused the module for a rule without its own code.
Fix: the message names the rule.

## Z0401 zeno-exec not available to the tests

Cause: `ZENO_EXEC` is unset, or `zeno-exec run` failed to start.
Fix: run tests with `cargo zeno test`

## Z0402 contract tests run outside cargo zeno test

Cause: a test that uses `TestDomain` ran without `ZENO_FRESH=1`, so the artifacts it reads may be stale.
Fix: run contract tests with `cargo zeno test`; run host tests with `cargo test --workspace --exclude <contract crates>`

## Z0403 method not in the interface

Cause: `call` named a method the contract's interface lacks, or the contract was not deployed by this `TestDomain`.
Fix: use a listed method, or `call_raw`

## Z0404 zeno-exec output not readable

Cause: `zeno-exec` exited, or wrote a line the harness cannot read.
Fix: read the stderr lines in the message

## Z0405 artifact file missing

Cause: `deploy` or `upgrade` could not read the artifact's `.wasm` or `.interface.json` file from the crate's `artifacts` directory.
Fix: list the artifact in `[package.metadata.zeno]` and run `cargo zeno test`

## Z0406 command refused

Cause: `zeno-exec` refused a command with a `halt` or `bad_command` result.
Fix: the message names the op and the reason

## Z0407 deploy failed

Cause: the deploy receipt is not `SUCCESS`.
Fix: the status and abort code in the message
