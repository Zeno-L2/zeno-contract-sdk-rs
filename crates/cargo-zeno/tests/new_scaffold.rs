//! Runs `cargo zeno new` and checks the files it writes.

use std::path::{Path, PathBuf};
use std::process::Command;

const SDK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn the_scaffold_copies_the_root_files() {
    let sdk = std::fs::canonicalize(SDK).unwrap();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("cargo-zeno-new-{}-{nanos}", std::process::id()));
    // Exclusive creation with mode 0700 fails if the path exists.
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700)
        .create(&work)
        .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_cargo-zeno"))
        .args(["new", "demo", "--sdk-path"])
        .arg(&sdk)
        .current_dir(&work)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let demo = work.join("demo");
    let mut all = Vec::new();
    files(&demo, &mut all);
    assert!(all.len() >= 7, "too few files: {all:?}");

    for name in ["LICENSE", "rust-toolchain.toml"] {
        assert_eq!(
            std::fs::read(demo.join(name)).unwrap(),
            std::fs::read(sdk.join(name)).unwrap(),
            "{name} differs from the root file"
        );
    }

    std::fs::remove_dir_all(&work).unwrap();
}
