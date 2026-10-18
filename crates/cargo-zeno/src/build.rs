//! Workspace loading, the wasm build and the reproducibility check.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::diag::{ArtifactReport, Diag, Report};
use crate::explain;
use crate::strip;
use crate::zeno_exec::{self, Validated};

pub struct Workspace {
    pub root: PathBuf,
    /// Sorted by package name.
    pub crates: Vec<ContractCrate>,
}

pub struct ContractCrate {
    pub package: String,
    pub dir: PathBuf,
    pub lib_name: String,
    pub artifacts: Vec<Artifact>,
}

pub struct Artifact {
    pub name: String,
    pub features: Vec<String>,
    pub metadata: Option<String>,
}

fn diag(code: &'static str, message: impl Into<String>) -> Diag {
    Diag::new(code, message, explain::fix(code))
}

/// The cargo binary: the `CARGO` variable, else `cargo`.
pub fn cargo_bin() -> PathBuf {
    std::env::var_os("CARGO")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

/// A cargo command whose output goes to stderr and whose environment
/// cannot override the checked profile.
pub fn cargo_command(cargo: &Path, dir: &Path) -> Command {
    let mut cmd = Command::new(cargo);
    cmd.current_dir(dir)
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::from(std::io::stderr()))
        .env_remove("CARGO_INCREMENTAL");
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CARGO_PROFILE_") {
            cmd.env_remove(k);
        }
    }
    cmd
}

pub fn load_workspace(cargo: &Path, cwd: &Path) -> Result<Workspace, Diag> {
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(cwd)
        .stderr(Stdio::from(std::io::stderr()))
        .output()
        .map_err(|e| diag("Z0209", format!("cannot run cargo metadata: {e}")))?;
    if !out.status.success() {
        return Err(diag(
            "Z0209",
            format!("cargo metadata exited with {}", out.status),
        ));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| diag("Z0209", format!("cargo metadata wrote invalid JSON: {e}")))?;
    let root = PathBuf::from(meta["workspace_root"].as_str().unwrap_or_default());
    if !root.join("Cargo.lock").is_file() {
        return Err(diag(
            "Z0204",
            format!("{} does not exist", root.join("Cargo.lock").display()),
        ));
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| {
        diag(
            "Z0209",
            format!("cannot read the workspace Cargo.toml: {e}"),
        )
    })?;
    check_profile(&manifest)?;

    let mut crates = Vec::new();
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        let table = &pkg["metadata"]["zeno"];
        if table.is_null() {
            continue;
        }
        let package = pkg["name"].as_str().unwrap_or_default().to_string();
        let artifacts = parse_zeno_table(&package, table)?;
        let lib_name = pkg["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| {
                t["crate_types"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c == "cdylib"))
            })
            .and_then(|t| t["name"].as_str())
            .map(|n| n.replace('-', "_"))
            .ok_or_else(|| diag("Z0208", format!("package `{package}` has no cdylib target")))?;
        let dir = Path::new(pkg["manifest_path"].as_str().unwrap_or_default())
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        crates.push(ContractCrate {
            package,
            dir,
            lib_name,
            artifacts,
        });
    }
    if crates.is_empty() {
        return Err(diag(
            "Z0208",
            "no workspace member has a [package.metadata.zeno] table",
        ));
    }
    crates.sort_by(|a, b| a.package.cmp(&b.package));
    let mut seen = BTreeSet::new();
    for a in crates.iter().flat_map(|c| &c.artifacts) {
        if !seen.insert(a.name.as_str()) {
            return Err(diag(
                "Z0208",
                format!("artifact name `{}` is used twice in the workspace", a.name),
            ));
        }
    }
    Ok(Workspace { root, crates })
}

/// The release profile every contract build must use.
fn required_profile() -> Vec<(&'static str, toml::Value)> {
    vec![
        ("opt-level", toml::Value::String("z".into())),
        ("lto", toml::Value::Boolean(true)),
        ("codegen-units", toml::Value::Integer(1)),
        ("panic", toml::Value::String("abort".into())),
        ("strip", toml::Value::Boolean(true)),
        ("debug", toml::Value::Boolean(false)),
    ]
}

pub fn check_profile(root_manifest: &str) -> Result<(), Diag> {
    let doc: toml::Table = toml::from_str(root_manifest).map_err(|e| {
        diag(
            "Z0209",
            format!("cannot parse the workspace Cargo.toml: {e}"),
        )
    })?;
    let release = doc
        .get("profile")
        .and_then(|p| p.get("release"))
        .and_then(|r| r.as_table())
        .cloned()
        .unwrap_or_default();
    let required = required_profile();
    for (key, want) in &required {
        match release.get(*key) {
            None => {
                return Err(Diag::new(
                    "Z0203",
                    format!("[profile.release] does not set `{key}`"),
                    format!("set `{key} = {want}` in [profile.release]"),
                ));
            }
            Some(got) if got != want => {
                return Err(Diag::new(
                    "Z0203",
                    format!("[profile.release] sets `{key}` to {got}, not {want}"),
                    format!("set `{key} = {want}` in [profile.release]"),
                ));
            }
            Some(_) => {}
        }
    }
    for key in release.keys() {
        if !required.iter().any(|(k, _)| k == key) {
            return Err(Diag::new(
                "Z0203",
                format!("[profile.release] sets `{key}`, which is not allowed"),
                format!("remove `{key}` from [profile.release]"),
            ));
        }
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn check_artifact_name(name: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!(
            "artifact name `{name}` does not match [a-z0-9][a-z0-9_-]*"
        ));
    }
    if name == "host" || name == "zeno-exec" {
        return Err(format!("artifact name `{name}` is reserved"));
    }
    Ok(())
}

pub fn parse_zeno_table(package: &str, table: &serde_json::Value) -> Result<Vec<Artifact>, Diag> {
    let bad = |why: String| diag("Z0208", format!("package `{package}`: {why}"));
    let Some(table) = table.as_object() else {
        return Err(bad("[package.metadata.zeno] is not a table".into()));
    };
    if let Some(key) = table.keys().find(|k| *k != "artifacts") {
        return Err(bad(format!(
            "unknown key `{key}` in [package.metadata.zeno]"
        )));
    }
    let Some(list) = table.get("artifacts") else {
        check_artifact_name(package).map_err(bad)?;
        return Ok(vec![Artifact {
            name: package.to_string(),
            features: Vec::new(),
            metadata: None,
        }]);
    };
    let Some(list) = list.as_array() else {
        return Err(bad("`artifacts` is not an array".into()));
    };
    let mut out = Vec::new();
    for entry in list {
        let Some(entry) = entry.as_object() else {
            return Err(bad("an `artifacts` entry is not a table".into()));
        };
        if let Some(key) = entry
            .keys()
            .find(|k| !["name", "features", "metadata"].contains(&k.as_str()))
        {
            return Err(bad(format!("unknown key `{key}` in an artifact")));
        }
        let Some(name) = entry.get("name").and_then(|n| n.as_str()) else {
            return Err(bad("an artifact needs a string `name`".into()));
        };
        check_artifact_name(name).map_err(bad)?;
        if out.iter().any(|a: &Artifact| a.name == name) {
            return Err(bad(format!("artifact name `{name}` is used twice")));
        }
        let mut features = Vec::new();
        if let Some(f) = entry.get("features") {
            let Some(f) = f.as_array() else {
                return Err(bad(format!("`features` of `{name}` is not an array")));
            };
            for item in f {
                let Some(s) = item.as_str() else {
                    return Err(bad(format!("`features` of `{name}` holds a non-string")));
                };
                features.push(s.to_string());
            }
        }
        let metadata = match entry.get("metadata") {
            None => None,
            Some(m) => match m.as_str() {
                Some(s) if s == "immutable" || s.starts_with("owner:") => Some(s.to_string()),
                _ => {
                    return Err(bad(format!(
                        "`metadata` of `{name}` must be \"immutable\" or \"owner:<address>\""
                    )));
                }
            },
        };
        out.push(Artifact {
            name: name.to_string(),
            features,
            metadata,
        });
    }
    Ok(out)
}

fn io_err(what: &str, path: &Path, e: std::io::Error) -> Diag {
    diag("Z0209", format!("cannot {what} {}: {e}", path.display()))
}

fn run_step(mut cmd: Command, what: &str) -> Result<(), Diag> {
    let status = cmd
        .status()
        .map_err(|e| diag("Z0209", format!("cannot run {what}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(diag("Z0209", format!("{what} exited with {status}")))
    }
}

fn feature_args(cmd: &mut Command, features: &[String]) {
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Builds, strips and validates every artifact, writes the staged files
/// under `target/zeno` and copies them into each crate's `artifacts`.
pub fn build(ws: &Workspace, exec: &Path, report: &mut Report) -> Result<(), Diag> {
    let cargo = cargo_bin();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .unwrap_or_default();
    let flags = [
        "-C".to_string(),
        "link-arg=--stack-first".into(),
        "-C".into(),
        "link-arg=-zstack-size=32768".into(),
        "-C".into(),
        "link-arg=--compress-relocations".into(),
        format!("--remap-path-prefix={}=/cargo", cargo_home.display()),
    ]
    .join("\x1f");
    let stage = ws.root.join("target/zeno");
    std::fs::create_dir_all(&stage).map_err(|e| io_err("create", &stage, e))?;

    for krate in &ws.crates {
        for art in &krate.artifacts {
            let name = &art.name;
            // 1. The wasm build.
            let mut cmd = cargo_command(&cargo, &ws.root);
            cmd.args([
                "build",
                "--release",
                "--locked",
                "--target",
                "wasm32v1-none",
                "-p",
                &krate.package,
            ]);
            feature_args(&mut cmd, &art.features);
            cmd.arg("--target-dir")
                .arg(stage.join(name))
                .env("CARGO_ENCODED_RUSTFLAGS", &flags);
            run_step(cmd, &format!("cargo build for artifact `{name}`"))?;

            // 2. Strip and hash.
            let built = stage
                .join(name)
                .join("wasm32v1-none/release")
                .join(format!("{}.wasm", krate.lib_name));
            let raw = std::fs::read(&built).map_err(|e| io_err("read", &built, e))?;
            let stripped = strip::strip(&raw)
                .map_err(|e| diag("Z0209", format!("cannot strip {}: {e}", built.display())))?;
            let code_hash = strip::code_hash(&stripped);
            let wasm = stage.join(format!("{name}.wasm"));
            std::fs::write(&wasm, &stripped).map_err(|e| io_err("write", &wasm, e))?;

            // 3. Validate.
            let v = zeno_exec::validate(exec, &wasm, art.metadata.as_deref())?;
            if v.code_hash != code_hash {
                return Err(diag(
                    "Z0209",
                    format!(
                        "the stripped module hashes to {code_hash} but zeno-exec reports {}",
                        v.code_hash
                    ),
                ));
            }

            // 4. The interface file.
            let iface = stage.join(format!("{name}.interface.json"));
            let _ = std::fs::remove_file(&iface);
            let mut cmd = cargo_command(&cargo, &ws.root);
            cmd.args(["test", "--lib", "--locked", "-p", &krate.package]);
            feature_args(&mut cmd, &art.features);
            cmd.arg("--target-dir")
                .arg(stage.join("host"))
                .args(["--", "__zeno_interface_file", "--exact"])
                .env("ZENO_INTERFACE_OUT", &iface)
                .env("ZENO_CODE_HASH", &code_hash)
                .env_remove("CARGO_ENCODED_RUSTFLAGS");
            let status = cmd
                .status()
                .map_err(|e| diag("Z0209", format!("cannot run cargo test: {e}")))?;
            if !status.success() {
                return Err(diag(
                    "Z0209",
                    format!("the interface test for `{name}` failed; its output is above"),
                ));
            }
            if !iface.is_file() {
                return Err(Diag::new(
                    "Z0209",
                    format!("the interface test did not run for `{name}`"),
                    "put the #[contract] module at the crate root (src/lib.rs)",
                ));
            }

            // 5. The hashes file and the committed copies.
            let hashes = stage.join(format!("{name}.hashes.json"));
            std::fs::write(&hashes, hashes_json(&v)).map_err(|e| io_err("write", &hashes, e))?;
            let dest = krate.dir.join("artifacts");
            std::fs::create_dir_all(&dest).map_err(|e| io_err("create", &dest, e))?;
            for ext in ["wasm", "interface.json", "hashes.json"] {
                let from = stage.join(format!("{name}.{ext}"));
                let to = dest.join(format!("{name}.{ext}"));
                std::fs::copy(&from, &to).map_err(|e| io_err("copy to", &to, e))?;
            }
            report.artifacts.push(ArtifactReport {
                name: name.clone(),
                wasm: rel(&ws.root, &dest.join(format!("{name}.wasm"))),
                code_hash: v.code_hash,
                instrumented_code_hash: v.instrumented_code_hash,
                size: v.size,
                instrumented_size: v.instrumented_size,
                initial_pages: v.initial_pages,
            });
        }
    }
    Ok(())
}

fn hashes_json(v: &Validated) -> String {
    let mut s = format!(
        "{{\"code_hash\":\"{}\",\"instrumented_code_hash\":\"{}\"",
        v.code_hash, v.instrumented_code_hash
    );
    if let (Some(m), Some(h)) = (&v.metadata, &v.metadata_hash) {
        s.push_str(&format!(",\"metadata\":\"{m}\",\"metadata_hash\":\"{h}\""));
    }
    s.push_str("}\n");
    s
}

/// Rebuilds the workspace in a scratch copy and compares every artifact
/// file with the one under the original root. Nothing is written under
/// the original root.
pub fn check(ws: &Workspace, cargo: &Path, exec: &Path, report: &mut Report) -> Result<(), Diag> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let copy = std::env::temp_dir().join(format!("zeno-check-{}-{nanos}", std::process::id()));
    create_private_dir(&copy)?;
    let result = check_in(ws, cargo, exec, report, &copy);
    let _ = std::fs::remove_dir_all(&copy);
    result
}

/// Creates `path` exclusively; on unix only its owner can use it.
fn create_private_dir(path: &Path) -> Result<(), Diag> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path).map_err(|e| io_err("create", path, e))
}

fn check_in(
    ws: &Workspace,
    cargo: &Path,
    exec: &Path,
    report: &mut Report,
    copy: &Path,
) -> Result<(), Diag> {
    copy_tree(&ws.root, copy, &ws.root.join("target"))?;
    let copied = load_workspace(cargo, copy)?;
    build(&copied, exec, report)?;
    for krate in &copied.crates {
        for art in &krate.artifacts {
            for ext in ["wasm", "interface.json", "hashes.json"] {
                let file = format!("artifacts/{}.{ext}", art.name);
                let new = krate.dir.join(&file);
                let original = ws
                    .root
                    .join(krate.dir.strip_prefix(&copied.root).unwrap_or(&krate.dir))
                    .join(&file);
                let want = std::fs::read(&new).map_err(|e| io_err("read", &new, e))?;
                match std::fs::read(&original) {
                    Ok(got) if got == want => {}
                    Ok(_) => report.errors.push(diag(
                        "Z0207",
                        format!("{} differs from a fresh build", original.display()),
                    )),
                    Err(_) => report
                        .errors
                        .push(diag("Z0207", format!("{} is missing", original.display()))),
                }
            }
        }
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, skip: &Path) -> Result<(), Diag> {
    std::fs::create_dir_all(to).map_err(|e| io_err("create", to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| io_err("read", from, e))? {
        let entry = entry.map_err(|e| io_err("read", from, e))?;
        let path = entry.path();
        if path == skip || entry.file_name() == ".git" {
            continue;
        }
        let dest = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| io_err("stat", &path, e))?;
        if kind.is_dir() {
            copy_tree(&path, &dest, skip)?;
        } else if kind.is_symlink() {
            copy_symlink(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest).map_err(|e| io_err("copy", &path, e))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(from: &Path, to: &Path) -> Result<(), Diag> {
    let target = std::fs::read_link(from).map_err(|e| io_err("read", from, e))?;
    std::os::unix::fs::symlink(target, to).map_err(|e| io_err("create", to, e))
}

#[cfg(not(unix))]
fn copy_symlink(from: &Path, to: &Path) -> Result<(), Diag> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| io_err("copy", from, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root_manifest() -> &'static str {
        include_str!("../../../Cargo.toml")
    }

    #[test]
    fn the_root_profile_passes() {
        assert!(check_profile(root_manifest()).is_ok());
    }

    #[test]
    fn profile_errors_name_the_key() {
        let changed = root_manifest().replace("opt-level = \"z\"", "opt-level = 3");
        let e = check_profile(&changed).unwrap_err();
        assert_eq!(e.code, "Z0203");
        assert!(e.message.contains("opt-level"));

        let missing = root_manifest().replace("lto = true\n", "");
        let e = check_profile(&missing).unwrap_err();
        assert!(e.message.contains("lto"));
        assert!(e.fix.contains("lto = true"));

        let extra = format!("{}incremental = true\n", root_manifest());
        let e = check_profile(&extra).unwrap_err();
        assert!(e.message.contains("incremental"));
        assert!(e.fix.contains("remove"));
    }

    #[test]
    fn an_absent_artifacts_key_names_one_artifact_after_the_package() {
        let a = parse_zeno_table("vault-shape", &json!({})).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].name, "vault-shape");
        assert!(a[0].features.is_empty() && a[0].metadata.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_private_dir_is_owner_only_and_never_reused() {
        use std::os::unix::fs::PermissionsExt;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("cargo-zeno-private-{}-{nanos}", std::process::id()));
        create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert!(create_private_dir(&dir).is_err());
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn a_defaulted_artifact_name_is_checked() {
        for package in ["host", "zeno-exec", "Upper"] {
            let e = parse_zeno_table(package, &json!({})).err().unwrap();
            assert_eq!(e.code, "Z0208", "{package}");
        }
    }

    #[test]
    fn features_and_metadata_are_read() {
        let t = json!({"artifacts": [
            {"name": "a"},
            {"name": "b-2", "features": ["v2"], "metadata": "owner:abc"},
        ]});
        let a = parse_zeno_table("p", &t).unwrap();
        assert_eq!(a[1].name, "b-2");
        assert_eq!(a[1].features, ["v2"]);
        assert_eq!(a[1].metadata.as_deref(), Some("owner:abc"));
    }

    #[test]
    fn malformed_tables_give_z0208() {
        for t in [
            json!({"other": 1}),
            json!({"artifacts": [{"name": "a", "other": 1}]}),
            json!({"artifacts": [{"name": "Bad"}]}),
            json!({"artifacts": [{"name": "host"}]}),
            json!({"artifacts": [{"name": "zeno-exec"}]}),
            json!({"artifacts": [{"name": "a", "metadata": 3}]}),
            json!({"artifacts": [{"name": "a", "metadata": "mutable"}]}),
            json!({"artifacts": [{"name": "a"}, {"name": "a"}]}),
        ] {
            assert_eq!(
                parse_zeno_table("p", &t).err().map(|d| d.code),
                Some("Z0208"),
                "{t}"
            );
        }
    }
}
