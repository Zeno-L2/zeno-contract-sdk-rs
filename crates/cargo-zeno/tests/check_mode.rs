//! Runs `cargo zeno build --check` in a copy of this workspace. It
//! needs a zeno-exec binary and the wasm toolchain: the test checks
//! nothing unless `ZENO_EXEC` is set, and fails instead when
//! `ZENO_REQUIRE_WASM_TESTS` is 1.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

type Snapshot = BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)>;

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if matches!(name.to_str(), Some("target" | ".git")) {
            continue;
        }
        let dest = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

fn snapshot(dir: &Path, out: &mut Snapshot) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            snapshot(&path, out);
        } else {
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            out.insert(path.clone(), (std::fs::read(&path).unwrap(), modified));
        }
    }
}

fn snap(dir: &Path) -> Snapshot {
    let mut s = Snapshot::new();
    snapshot(dir, &mut s);
    s
}

fn run_check(dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-zeno"))
        .args(["build", "--check"])
        .current_dir(dir)
        .output()
        .unwrap()
}

fn z0207_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|l| l.starts_with("error[Z0207]"))
        .map(String::from)
        .collect()
}

#[test]
fn check_reports_differences_and_never_writes_to_the_workspace() {
    if std::env::var_os("ZENO_EXEC").is_none_or(|v| v.is_empty()) {
        assert_ne!(
            std::env::var("ZENO_REQUIRE_WASM_TESTS").as_deref(),
            Ok("1"),
            "ZENO_EXEC is not set"
        );
        return;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!(
        "cargo-zeno-check-mode-{}-{nanos}",
        std::process::id()
    ));
    // Exclusive creation with mode 0700 fails if the path exists.
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700)
        .create(&work)
        .unwrap();
    copy_tree(&std::fs::canonicalize(ROOT).unwrap(), &work);

    // Committed artifacts match a fresh build, and nothing changes.
    let before = snap(&work);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let out = run_check(&work);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(z0207_lines(&out).is_empty());
    assert!(snap(&work) == before, "a passing check changed the tree");

    // One altered and one missing artifact, each reported once.
    let altered = work.join("examples/counter/artifacts/counter.interface.json");
    let missing = work.join("examples/counter/artifacts/counter-v2.hashes.json");
    let mut bytes = std::fs::read(&altered).unwrap();
    bytes.push(b'x');
    std::fs::write(&altered, bytes).unwrap();
    std::fs::remove_file(&missing).unwrap();
    let before = snap(&work);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let out = run_check(&work);
    assert_eq!(out.status.code(), Some(1));
    let lines = z0207_lines(&out);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("counter.interface.json")
                && l.contains("differs from a fresh build")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("counter-v2.hashes.json") && l.contains("is missing")),
        "{lines:?}"
    );
    assert!(snap(&work) == before, "a failing check changed the tree");

    std::fs::remove_dir_all(&work).unwrap();
}
