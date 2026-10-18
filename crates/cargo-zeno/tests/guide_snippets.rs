//! Extends a fresh scaffold with the marked snippets of
//! `docs/guide.md` and runs its tests. It needs a zeno-exec binary,
//! the wasm toolchain and the crates.io index: the test checks nothing
//! unless `ZENO_EXEC` is set, and fails instead when
//! `ZENO_REQUIRE_WASM_TESTS` is 1.

use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

const ERRORS_ANCHOR: &str = "        NotHigher = 1,\n";
const MODULE_ANCHOR: &str = "\n}\n\n#[cfg(test)]\nmod tests;\n";

/// The rust blocks whose opening fence directly follows
/// `<!-- guide-test: {kind} -->`.
fn blocks(guide: &str, kind: &str) -> Vec<String> {
    let marker = format!("<!-- guide-test: {kind} -->");
    let lines: Vec<&str> = guide.lines().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i] == marker && lines[i + 1] == "```rust" {
            let mut block = String::new();
            i += 2;
            while i < lines.len() && lines[i] != "```" {
                block.push_str(lines[i]);
                block.push('\n');
                i += 1;
            }
            found.push(block);
        }
        i += 1;
    }
    assert!(!found.is_empty(), "no block follows the {marker} marker");
    found
}

fn insert_once(text: &str, anchor: &str, at: usize, insert: &str) -> String {
    assert_eq!(
        text.matches(anchor).count(),
        1,
        "the anchor {anchor:?} must occur exactly once"
    );
    let pos = text.find(anchor).unwrap() + at;
    format!("{}{insert}{}", &text[..pos], &text[pos..])
}

#[test]
fn guide_snippets_extend_a_scaffold() {
    if std::env::var_os("ZENO_EXEC").is_none_or(|v| v.is_empty()) {
        assert_ne!(
            std::env::var("ZENO_REQUIRE_WASM_TESTS").as_deref(),
            Ok("1"),
            "ZENO_EXEC is not set"
        );
        return;
    }
    let root = std::fs::canonicalize(ROOT).unwrap();
    let guide = std::fs::read_to_string(root.join("docs/guide.md")).unwrap();
    let errors = blocks(&guide, "errors");
    let module = blocks(&guide, "module");
    let tests = blocks(&guide, "tests");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work =
        std::env::temp_dir().join(format!("cargo-zeno-guide-{}-{nanos}", std::process::id()));
    // Exclusive creation with mode 0700 fails if the path exists.
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700)
        .create(&work)
        .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_cargo-zeno"))
        .args(["new", "demo", "--sdk-path"])
        .arg(&root)
        .current_dir(&work)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let demo = work.join("demo");
    let lib_path = demo.join("src/lib.rs");
    let mut lib = std::fs::read_to_string(&lib_path).unwrap();
    lib = insert_once(&lib, ERRORS_ANCHOR, ERRORS_ANCHOR.len(), &errors.concat());
    let module_text: String = module.iter().map(|b| format!("\n{b}")).collect();
    lib = insert_once(&lib, MODULE_ANCHOR, 1, &module_text);
    std::fs::write(&lib_path, lib).unwrap();

    let tests_path = demo.join("src/tests.rs");
    let mut tests_file = std::fs::read_to_string(&tests_path).unwrap();
    let mut added = 0;
    for b in &tests {
        added += b.matches("#[test]").count();
        tests_file.push('\n');
        tests_file.push_str(b);
    }
    std::fs::write(&tests_path, tests_file).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_cargo-zeno"))
        .args(["test", "--json"])
        .current_dir(&demo)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "test failed: {stdout}\n{stderr}");
    let report: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(report["ok"], true, "{stdout}");
    assert_eq!(report["tests"]["failed"], serde_json::json!([]), "{stdout}");
    // The two template tests, the generated interface test and the
    // snippet tests.
    assert_eq!(report["tests"]["passed"], 3 + added, "{stdout}");

    std::fs::remove_dir_all(&work).unwrap();
}
