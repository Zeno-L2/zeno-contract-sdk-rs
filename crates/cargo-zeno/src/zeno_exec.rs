//! Finds, checks and runs the `zeno-exec` binary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::diag::Diag;
use crate::explain;

#[derive(Deserialize)]
pub struct Pin {
    pub version: String,
    pub revision: String,
    pub sha256: BTreeMap<String, String>,
}

pub fn pin() -> Pin {
    toml::from_str(include_str!("../zeno-exec.pin")).expect("zeno-exec.pin parses")
}

/// The release platform name of this machine, if any.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Some("darwin-amd64"),
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        _ => None,
    }
}

fn diag(code: &'static str, message: impl Into<String>) -> Diag {
    Diag::new(code, message, explain::fix(code))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks the file against a SHA-256 in hex. A mismatch deletes the
/// file.
pub fn verify_sha256(path: &Path, want_hex: &str) -> Result<(), Diag> {
    let bytes = std::fs::read(path)
        .map_err(|e| diag("Z0209", format!("cannot read {}: {e}", path.display())))?;
    let got = hex(&Sha256::digest(&bytes));
    if got.eq_ignore_ascii_case(want_hex) {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Err(diag(
        "Z0202",
        format!(
            "{} has SHA-256 {got}, the pin wants {want_hex}",
            path.display()
        ),
    ))
}

/// Finds `zeno-exec`. Each step reports one note; the first step that
/// yields a binary wins.
pub fn resolve(root: &Path, pin: &Pin, note: &mut dyn FnMut(String)) -> Result<PathBuf, Diag> {
    // Step 1: the ZENO_EXEC variable.
    match std::env::var_os("ZENO_EXEC").filter(|v| !v.is_empty()) {
        None => note("zeno-exec: ZENO_EXEC skipped: not set".into()),
        Some(v) => {
            let path = std::path::absolute(&v).unwrap_or_else(|_| PathBuf::from(&v));
            if path.is_file() {
                note(format!("zeno-exec: ZENO_EXEC used: {}", path.display()));
                return Ok(path);
            }
            note(format!(
                "zeno-exec: ZENO_EXEC failed: {} is not a file",
                path.display()
            ));
        }
    }

    let pinned = platform().and_then(|p| pin.sha256.get(p).map(|sum| (p, sum.as_str())));
    let skip_reason = match (platform(), pinned) {
        (None, _) => "unsupported platform".to_string(),
        (Some(p), None) => format!("no checksum pinned for {p}"),
        _ => String::new(),
    };
    let cache_dir = std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".cache/zeno/zeno-exec")
            .join(&pin.version)
    });

    // Steps 2 and 3: the cache and the pinned download.
    match (pinned, &cache_dir) {
        (Some((platform, sum)), Some(dir)) => {
            if let Some(cached) = from_cache(dir, sum, note) {
                return Ok(cached);
            }
            match download(dir, &pin.version, platform, sum) {
                Ok(path) => {
                    note(format!("zeno-exec: download used: {}", path.display()));
                    return Ok(path);
                }
                Err(Fetch::Mismatch(d)) => return Err(d),
                Err(Fetch::Failed(why)) => note(format!("zeno-exec: download failed: {why}")),
            }
        }
        _ => {
            let why = if pinned.is_none() {
                skip_reason
            } else {
                "HOME is not set".into()
            };
            note(format!("zeno-exec: cache skipped: {why}"));
            note(format!("zeno-exec: download skipped: {why}"));
        }
    }

    // Step 4: the sibling zeno-executor checkout.
    let sibling = root.join("../zeno-executor");
    if sibling.join("cmd/zeno-exec").is_dir() {
        let out = root.join("target/zeno/zeno-exec");
        let built = std::fs::create_dir_all(root.join("target/zeno"))
            .map_err(|e| e.to_string())
            .and_then(|()| {
                Command::new("go")
                    .arg("-C")
                    .arg(&sibling)
                    .args(["build", "-o"])
                    .arg(&out)
                    .arg("./cmd/zeno-exec")
                    .stdout(Stdio::from(std::io::stderr()))
                    .stderr(Stdio::from(std::io::stderr()))
                    .status()
                    .map_err(|e| format!("cannot run go: {e}"))
            })
            .and_then(|s| {
                if s.success() {
                    Ok(())
                } else {
                    Err(format!("go build exited with {s}"))
                }
            });
        match built {
            Ok(()) => {
                note(format!("zeno-exec: sibling used: {}", out.display()));
                return Ok(out);
            }
            Err(why) => note(format!("zeno-exec: sibling failed: {why}")),
        }
    } else {
        note(format!(
            "zeno-exec: sibling skipped: {} is not a zeno-executor checkout",
            sibling.display()
        ));
    }

    Err(diag(
        "Z0201",
        format!(
            "no zeno-exec found: set ZENO_EXEC, pin a release (https://github.com/Zeno-L2/zeno-executor/releases/download/zeno-exec-v{}/zeno-exec-<platform>) or check out zeno-executor at {}",
            pin.version,
            sibling.display()
        ),
    ))
}

/// Returns the cached binary in `dir` when it matches the pinned
/// checksum. A mismatching file is deleted.
fn from_cache(dir: &Path, sum: &str, note: &mut dyn FnMut(String)) -> Option<PathBuf> {
    let cached = dir.join("zeno-exec");
    if !cached.is_file() {
        note(format!(
            "zeno-exec: cache skipped: {} is absent",
            cached.display()
        ));
        return None;
    }
    if verify_sha256(&cached, sum).is_ok() {
        note(format!("zeno-exec: cache used: {}", cached.display()));
        return Some(cached);
    }
    note(format!(
        "zeno-exec: cache failed: {} does not match the pinned checksum, deleted",
        cached.display()
    ));
    None
}

enum Fetch {
    Mismatch(Diag),
    Failed(String),
}

/// Downloads the release binary into memory, checks it against the
/// pinned checksum and installs exactly those bytes with an atomic
/// rename.
fn download(dir: &Path, version: &str, platform: &str, sum: &str) -> Result<PathBuf, Fetch> {
    let fail = |e: String| Fetch::Failed(e);
    std::fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
    let url = format!(
        "https://github.com/Zeno-L2/zeno-executor/releases/download/zeno-exec-v{version}/zeno-exec-{platform}"
    );
    // -q comes first so curl ignores the user's .curlrc.
    let out = Command::new("curl")
        .args([
            "-q",
            "-fsSL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "300",
            "--max-filesize",
            "268435456",
        ])
        .arg(&url)
        .stderr(Stdio::from(std::io::stderr()))
        .output()
        .map_err(|e| fail(format!("cannot run curl: {e}")))?;
    if !out.status.success() {
        return Err(fail(format!("curl exited with {} for {url}", out.status)));
    }
    let got = hex(&Sha256::digest(&out.stdout));
    if !got.eq_ignore_ascii_case(sum) {
        return Err(Fetch::Mismatch(diag(
            "Z0202",
            format!("the download of {url} has SHA-256 {got}, the pin wants {sum}"),
        )));
    }
    let part = dir.join(format!("zeno-exec.{}.part", std::process::id()));
    write_executable(&part, &out.stdout).map_err(|e| fail(e.to_string()))?;
    let target = dir.join("zeno-exec");
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        fail(e.to_string())
    })?;
    Ok(target)
}

/// Creates `path` exclusively with mode 0755 and writes `bytes`.
fn write_executable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o755);
    let mut f = opts.open(path)?;
    f.write_all(bytes)
}

#[derive(Deserialize)]
pub struct Version {
    pub version: String,
    pub revision: String,
    pub modified: bool,
}

pub fn version(exec: &Path) -> Result<Version, Diag> {
    let out = Command::new(exec)
        .arg("version")
        .stderr(Stdio::from(std::io::stderr()))
        .output()
        .map_err(|e| {
            diag(
                "Z0209",
                format!("cannot run {} version: {e}", exec.display()),
            )
        })?;
    if !out.status.success() {
        return Err(diag(
            "Z0209",
            format!("{} version exited with {}", exec.display(), out.status),
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| {
        diag(
            "Z0209",
            format!(
                "{} version wrote output that is not valid: {e}",
                exec.display()
            ),
        )
    })
}

/// Returns the warnings and, when the revision differs under `strict`,
/// the error.
pub fn check_revision(v: &Version, pin: &Pin, strict: bool) -> (Vec<Diag>, Option<Diag>) {
    let mut warnings = Vec::new();
    let mut error = None;
    if v.revision != pin.revision {
        let d = diag(
            "Z0205",
            format!(
                "zeno-exec {} has revision {}, the pin is {}",
                v.version, v.revision, pin.revision
            ),
        );
        if strict {
            error = Some(d);
        } else {
            warnings.push(d);
        }
    }
    if v.modified {
        warnings.push(diag("Z0206", "zeno-exec was built from a modified tree"));
    }
    (warnings, error)
}

pub struct Validated {
    pub code_hash: String,
    pub instrumented_code_hash: String,
    pub size: u64,
    pub instrumented_size: u64,
    pub initial_pages: u32,
    pub metadata: Option<String>,
    pub metadata_hash: Option<String>,
}

pub fn validate(exec: &Path, module: &Path, metadata: Option<&str>) -> Result<Validated, Diag> {
    let mut cmd = Command::new(exec);
    cmd.arg("validate");
    if let Some(m) = metadata {
        cmd.args(["--metadata", m]);
    }
    let out = cmd.arg(module).output().map_err(|e| {
        diag(
            "Z0209",
            format!("cannot run {} validate: {e}", exec.display()),
        )
    })?;
    let bad_output = |e: String| {
        diag(
            "Z0209",
            format!(
                "{} validate wrote output that is not valid: {e}",
                exec.display()
            ),
        )
    };
    match out.status.code() {
        Some(0) => {
            let v: serde_json::Value =
                serde_json::from_slice(&out.stdout).map_err(|e| bad_output(e.to_string()))?;
            let text = |k: &str| v[k].as_str().map(str::to_string);
            let num = |k: &str| v[k].as_u64();
            let (
                Some(code_hash),
                Some(instrumented_code_hash),
                Some(size),
                Some(instrumented_size),
                Some(pages),
            ) = (
                text("codeHash"),
                text("instrumentedCodeHash"),
                num("size"),
                num("instrumentedSize"),
                num("initialPages"),
            )
            else {
                return Err(bad_output("a member is missing".into()));
            };
            Ok(Validated {
                code_hash,
                instrumented_code_hash,
                size,
                instrumented_size,
                initial_pages: pages as u32,
                metadata: text("metadata"),
                metadata_hash: text("metadataHash"),
            })
        }
        Some(1) => {
            let v: serde_json::Value =
                serde_json::from_slice(&out.stdout).map_err(|e| bad_output(e.to_string()))?;
            let kind = v["error"]["kind"].as_str().unwrap_or("");
            let message = v["error"]["message"].as_str().unwrap_or("");
            let code = kind_code(kind);
            Err(diag(code, message))
        }
        Some(2) => Err(diag(
            "Z0208",
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        )),
        _ => Err(diag(
            "Z0209",
            format!("{} validate exited with {}", exec.display(), out.status),
        )),
    }
}

/// Maps a `zeno-exec` error kind to its catalogue code.
pub fn kind_code(kind: &str) -> &'static str {
    match kind {
        "float_type" | "float_instruction" => "Z0301",
        "disallowed_import" | "import_signature" => "Z0302",
        "sign_extension"
        | "bulk_memory"
        | "reference_type"
        | "multi_value_result"
        | "simd"
        | "saturating_conversion"
        | "atomics"
        | "exception_handling"
        | "typed_select" => "Z0303",
        "module_too_large" | "instrumented_too_large" => "Z0304",
        "engine_compile" | "internal_panic" => "Z0305",
        _ => "Z0399",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_parses() {
        let p = pin();
        assert_eq!(p.version, "0.1.0");
        assert_eq!(p.revision.len(), 40);
        assert!(p.revision.bytes().all(|b| b.is_ascii_hexdigit()));
        let platforms: Vec<&str> = p.sha256.keys().map(String::as_str).collect();
        assert_eq!(
            platforms,
            ["darwin-amd64", "darwin-arm64", "linux-amd64", "linux-arm64"]
        );
        for sum in p.sha256.values() {
            assert_eq!(sum.len(), 64);
            assert!(sum.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    fn temp_file(tag: &str, bytes: &[u8]) -> PathBuf {
        use std::io::Write;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("cargo-zeno-{tag}-{}-{nanos}", std::process::id()));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    #[test]
    fn verify_sha256_keeps_a_match_and_deletes_a_mismatch() {
        let sum = hex(&Sha256::digest(b"abc"));
        let ok = temp_file("sum-ok", b"abc");
        assert!(verify_sha256(&ok, &sum).is_ok());
        assert!(ok.exists());
        std::fs::remove_file(&ok).unwrap();

        let bad = temp_file("sum-bad", b"abd");
        let err = verify_sha256(&bad, &sum).unwrap_err();
        assert_eq!(err.code, "Z0202");
        assert!(!bad.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_installed_binary_is_executable_and_never_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_file("install", b"");
        std::fs::remove_file(&path).unwrap();
        write_executable(&path, b"abc").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o700, 0o700);
        let e = write_executable(&path, b"xyz").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn the_cache_is_used_only_when_its_checksum_matches() {
        let file = temp_file("cache", b"");
        std::fs::remove_file(&file).unwrap();
        let dir = file.with_extension("d");
        std::fs::DirBuilder::new().create(&dir).unwrap();
        let bin = dir.join("zeno-exec");
        let sum = hex(&Sha256::digest(b"abc"));
        let mut notes = Vec::new();
        assert_eq!(from_cache(&dir, &sum, &mut |n| notes.push(n)), None);
        std::fs::write(&bin, b"abc").unwrap();
        assert_eq!(
            from_cache(&dir, &sum, &mut |n| notes.push(n)),
            Some(bin.clone())
        );
        std::fs::write(&bin, b"abd").unwrap();
        assert_eq!(from_cache(&dir, &sum, &mut |n| notes.push(n)), None);
        assert!(!bin.exists());
        assert!(notes[2].contains("cache failed"), "{notes:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn kind_code_follows_the_table() {
        for (kind, code) in [
            ("float_type", "Z0301"),
            ("float_instruction", "Z0301"),
            ("disallowed_import", "Z0302"),
            ("import_signature", "Z0302"),
            ("sign_extension", "Z0303"),
            ("bulk_memory", "Z0303"),
            ("reference_type", "Z0303"),
            ("multi_value_result", "Z0303"),
            ("simd", "Z0303"),
            ("saturating_conversion", "Z0303"),
            ("atomics", "Z0303"),
            ("exception_handling", "Z0303"),
            ("typed_select", "Z0303"),
            ("module_too_large", "Z0304"),
            ("instrumented_too_large", "Z0304"),
            ("engine_compile", "Z0305"),
            ("internal_panic", "Z0305"),
            ("bad_magic", "Z0399"),
        ] {
            assert_eq!(kind_code(kind), code, "{kind}");
        }
    }

    #[test]
    fn check_revision_warns_or_fails() {
        let p = pin();
        let same = Version {
            version: "devel".into(),
            revision: p.revision.clone(),
            modified: false,
        };
        assert_eq!(check_revision(&same, &p, true), (vec![], None));

        let other = Version {
            revision: "0".repeat(40),
            ..same
        };
        let (warnings, error) = check_revision(&other, &p, false);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "Z0205");
        assert!(error.is_none());
        let (warnings, error) = check_revision(&other, &p, true);
        assert!(warnings.is_empty());
        assert_eq!(error.unwrap().code, "Z0205");

        let dirty = Version {
            modified: true,
            ..other
        };
        let (warnings, _) = check_revision(&dirty, &p, true);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "Z0206");
    }
}
