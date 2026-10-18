//! Diagnostics and the report `--json` prints.

use serde::Serialize;

/// One reportable problem: a catalogue code, what happened and the fix.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diag {
    pub code: &'static str,
    pub message: String,
    pub fix: String,
}

impl Diag {
    pub fn new(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Diag {
        Diag {
            code,
            message: message.into(),
            fix: fix.into(),
        }
    }

    /// `<level>[<code>]: <message>` then `fix: <fix>` on the next line.
    pub fn render(&self, level: &str) -> String {
        format!(
            "{level}[{}]: {}\nfix: {}",
            self.code, self.message, self.fix
        )
    }
}

#[derive(Default, Serialize)]
pub struct Report {
    pub ok: bool,
    pub artifacts: Vec<ArtifactReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tests: Option<TestReport>,
    pub warnings: Vec<Diag>,
    pub errors: Vec<Diag>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReport {
    pub name: String,
    pub wasm: String,
    pub code_hash: String,
    pub instrumented_code_hash: String,
    pub size: u64,
    pub instrumented_size: u64,
    pub initial_pages: u32,
}

#[derive(Default, Serialize)]
pub struct TestReport {
    pub passed: u32,
    pub failed: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_puts_the_fix_on_its_own_line() {
        let d = Diag::new(
            "Z0204",
            "Cargo.lock is missing",
            "run cargo generate-lockfile",
        );
        assert_eq!(
            d.render("error"),
            "error[Z0204]: Cargo.lock is missing\nfix: run cargo generate-lockfile"
        );
        assert!(d.render("warning").starts_with("warning[Z0204]: "));
    }
}
