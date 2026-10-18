//! `cargo zeno test`: builds the artifacts, then runs each contract
//! crate's tests against `zeno-exec`.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;

use crate::build::{self, Workspace, cargo_bin, cargo_command};
use crate::diag::{Diag, Report, TestReport};
use crate::explain;

/// `test <name> ... ok` gives `(name, true)` and `... FAILED` gives
/// `(name, false)`. Any other line gives `None`.
fn parse_test_line(line: &str) -> Option<(&str, bool)> {
    let rest = line.strip_prefix("test ")?;
    if let Some(name) = rest.strip_suffix(" ... ok") {
        Some((name, true))
    } else {
        rest.strip_suffix(" ... FAILED").map(|name| (name, false))
    }
}

pub fn run_tests(ws: &Workspace, exec: &Path, report: &mut Report) -> Result<(), Diag> {
    build::build(ws, exec, report)?;
    let cargo = cargo_bin();
    let exec = std::path::absolute(exec).unwrap_or_else(|_| exec.to_path_buf());
    let mut tests = TestReport::default();
    let mut broken = Vec::new();
    for krate in &ws.crates {
        let mut cmd = cargo_command(&cargo, &ws.root);
        cmd.args(["test", "--lib", "--locked", "-p", &krate.package])
            .arg("--target-dir")
            .arg(ws.root.join("target/zeno/host"))
            .env("ZENO_EXEC", &exec)
            .env("ZENO_FRESH", "1")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .stdout(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            Diag::new(
                "Z0209",
                format!("cannot run cargo test: {e}"),
                explain::fix("Z0209"),
            )
        })?;
        let mut failed_here = false;
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let _ = writeln!(std::io::stderr(), "{line}");
                match parse_test_line(&line) {
                    Some((_, true)) => tests.passed += 1,
                    Some((name, false)) => {
                        tests.failed.push(format!("{}::{name}", krate.package));
                        failed_here = true;
                    }
                    None => {}
                }
            }
        }
        let status = child.wait().map_err(|e| {
            Diag::new(
                "Z0209",
                format!("cannot wait for cargo test: {e}"),
                explain::fix("Z0209"),
            )
        })?;
        if failed_here || !status.success() {
            broken.push(krate.package.clone());
        }
    }
    report.tests = Some(tests);
    if broken.is_empty() {
        Ok(())
    } else {
        Err(Diag::new(
            "Z0213",
            format!("tests failed in {}", broken.join(", ")),
            explain::fix("Z0213"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lines_are_read() {
        assert_eq!(
            parse_test_line("test tests::a ... ok"),
            Some(("tests::a", true))
        );
        assert_eq!(
            parse_test_line("test tests::b ... FAILED"),
            Some(("tests::b", false))
        );
        assert_eq!(parse_test_line("test tests::c ... ignored"), None);
        assert_eq!(parse_test_line("running 3 tests"), None);
        assert_eq!(parse_test_line("test result: ok. 3 passed"), None);
    }
}
