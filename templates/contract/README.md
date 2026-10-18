# {{name}}

A Zeno WASM contract built with `zeno-contract`.

- `cargo zeno build` builds, strips and validates the module with the link flags applied, and writes it to `artifacts/`. Use it instead of `cargo build`.
- `cargo zeno test` rebuilds the artifacts and runs the tests against `zeno-exec`.
- `cargo zeno explain <code>` prints the cause and fix of an error code.

`cargo zeno` needs `zeno-exec`. It uses `ZENO_EXEC`, then the cached pinned release, then a download of that release, then a build of `../zeno-executor`.
