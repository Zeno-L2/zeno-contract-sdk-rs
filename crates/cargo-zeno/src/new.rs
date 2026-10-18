//! `cargo zeno new`: writes a contract project from the embedded
//! template.

use std::path::{Path, PathBuf};

use crate::build::cargo_command;
use crate::diag::Diag;
use crate::explain;

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "abstract", "become", "box", "do", "final", "gen", "macro", "override",
    "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// Names that collide with a generated item or a dependency.
const RESERVED: &[&str] = &["test", "tests", "host", "zeno_contract", "zeno_testing"];

fn usage_diag(message: impl Into<String>) -> Diag {
    Diag::new("Z0211", message, explain::fix("Z0211"))
}

fn tool_diag(message: impl Into<String>, fix: impl Into<String>) -> Diag {
    Diag::new("Z0209", message, fix)
}

fn check_name(name: &str) -> Result<(), Diag> {
    let mut chars = name.chars();
    let shaped = matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !shaped {
        return Err(usage_diag(format!(
            "`{name}` does not match [a-z][a-z0-9_]*"
        )));
    }
    if KEYWORDS.contains(&name) {
        return Err(usage_diag(format!("`{name}` is a Rust keyword")));
    }
    if RESERVED.contains(&name) {
        return Err(usage_diag(format!(
            "`{name}` collides with a generated name"
        )));
    }
    Ok(())
}

/// Returns the project files as (relative path, contents).
fn render(name: &str, contract_dep: &str, testing_dep: &str) -> Vec<(&'static str, String)> {
    let fill = |text: &str| {
        text.replace("{{name}}", name)
            .replace("{{zeno-contract}}", contract_dep)
            .replace("{{zeno-testing}}", testing_dep)
    };
    vec![
        (
            "Cargo.toml",
            fill(include_str!("../../../templates/contract/Cargo.toml.tmpl")),
        ),
        (
            "src/lib.rs",
            fill(include_str!("../../../templates/contract/src/lib.rs")),
        ),
        (
            "src/tests.rs",
            fill(include_str!("../../../templates/contract/src/tests.rs")),
        ),
        (
            ".gitignore",
            fill(include_str!("../../../templates/contract/.gitignore")),
        ),
        (
            "README.md",
            fill(include_str!("../../../templates/contract/README.md")),
        ),
        (
            "rust-toolchain.toml",
            include_str!("../../../rust-toolchain.toml").to_string(),
        ),
        ("LICENSE", include_str!("../../../LICENSE").to_string()),
    ]
}

/// A TOML basic string.
fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

pub fn new_project(
    cwd: &Path,
    name: &str,
    sdk_path: Option<&Path>,
    cargo: &Path,
) -> Result<PathBuf, Diag> {
    check_name(name)?;
    let dir = cwd.join(name);
    if dir.exists() {
        return Err(usage_diag(format!("{} already exists", dir.display())));
    }
    let (contract_dep, testing_dep) = match sdk_path {
        None => {
            let git = format!(
                "{{ git = \"https://github.com/Zeno-L2/zeno-contract-sdk-rs\", tag = \"v{}\" }}",
                env!("CARGO_PKG_VERSION")
            );
            (git.clone(), git)
        }
        Some(p) => {
            let sdk = cwd.join(p).canonicalize().map_err(|e| {
                usage_diag(format!("--sdk-path {} is not usable: {e}", p.display()))
            })?;
            if !sdk.join("crates/zeno-contract/Cargo.toml").is_file() {
                return Err(usage_diag(format!(
                    "{} does not hold crates/zeno-contract",
                    sdk.display()
                )));
            }
            let dep = |krate: &str| {
                format!(
                    "{{ path = {} }}",
                    toml_string(&sdk.join("crates").join(krate).to_string_lossy())
                )
            };
            (dep("zeno-contract"), dep("zeno-testing"))
        }
    };
    for (rel, contents) in render(name, &contract_dep, &testing_dep) {
        let path = dir.join(rel);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, contents));
        written.map_err(|e| {
            tool_diag(
                format!("cannot write {}: {e}", path.display()),
                explain::fix("Z0209"),
            )
        })?;
    }
    let status = cargo_command(cargo, &dir).arg("generate-lockfile").status();
    let lock_fix = format!("run cargo generate-lockfile in {}", dir.display());
    match status {
        Ok(s) if s.success() => Ok(dir),
        Ok(s) => Err(tool_diag(
            format!("cargo generate-lockfile exited with {s}"),
            lock_fix,
        )),
        Err(e) => Err(tool_diag(
            format!("cannot run cargo generate-lockfile: {e}"),
            lock_fix,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_scaffold_parses_and_has_no_placeholders() {
        let dep = |k: &str| format!("{{ path = {} }}", toml_string(&format!("/sdk/crates/{k}")));
        let files = render("demo", &dep("zeno-contract"), &dep("zeno-testing"));
        for (path, text) in &files {
            assert!(!text.contains("{{"), "{path} keeps a placeholder");
        }
        let manifest = &files.iter().find(|(p, _)| *p == "Cargo.toml").unwrap().1;
        let doc: toml::Table = toml::from_str(manifest).unwrap();
        assert_eq!(doc["package"]["name"].as_str(), Some("demo"));
        assert_eq!(
            doc["dependencies"]["zeno-contract"]["path"].as_str(),
            Some("/sdk/crates/zeno-contract")
        );
        assert_eq!(
            doc["dev-dependencies"]["zeno-testing"]["path"].as_str(),
            Some("/sdk/crates/zeno-testing")
        );
        assert!(files.iter().any(|(p, _)| *p == "LICENSE"));
    }

    #[test]
    fn bad_names_are_refused() {
        for name in [
            "Demo",
            "1x",
            "my-token",
            "type",
            "test",
            "tests",
            "host",
            "zeno_contract",
            "",
        ] {
            assert_eq!(
                check_name(name).err().map(|d| d.code),
                Some("Z0211"),
                "{name}"
            );
        }
        assert!(check_name("my_token2").is_ok());
    }
}
