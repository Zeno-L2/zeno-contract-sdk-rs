//! `cargo zeno`: builds, tests and explains Zeno WASM contracts.

mod build;
mod diag;
mod explain;
mod new;
mod strip;
mod test;
mod zeno_exec;

use std::process::ExitCode;

use diag::{Diag, Report};

const USAGE: &str = "usage:
  cargo zeno new <name> [--sdk-path <path>]
  cargo zeno build [--check] [--json]
  cargo zeno test [--json]
  cargo zeno explain <code>
  cargo zeno -h | --help
";

enum Command {
    New {
        name: String,
        sdk_path: Option<String>,
    },
    Build {
        check: bool,
    },
    Test,
    Explain(String),
    Help,
}

fn usage_error(message: String) -> Diag {
    Diag::new("Z0210", message, explain::fix("Z0210"))
}

/// Parses the arguments after the program name. A leading `zeno` is
/// skipped, so both `cargo zeno <cmd>` and the alias work.
fn parse(args: &[String]) -> Result<(Command, bool), Diag> {
    let args = match args {
        [first, rest @ ..] if first == "zeno" => rest,
        all => all,
    };
    let Some((cmd, rest)) = args.split_first() else {
        return Err(usage_error("a command is required".into()));
    };
    let flags = |allowed: &[&str]| -> Result<Vec<String>, Diag> {
        for a in rest {
            if !allowed.contains(&a.as_str()) {
                return Err(usage_error(format!(
                    "unexpected argument `{a}` for `{cmd}`"
                )));
            }
        }
        Ok(rest.to_vec())
    };
    match cmd.as_str() {
        "-h" | "--help" => Ok((Command::Help, false)),
        "new" => match rest {
            [name] => Ok((
                Command::New {
                    name: name.clone(),
                    sdk_path: None,
                },
                false,
            )),
            [name, flag, path] if flag == "--sdk-path" => Ok((
                Command::New {
                    name: name.clone(),
                    sdk_path: Some(path.clone()),
                },
                false,
            )),
            _ => Err(usage_error(
                "new takes a name and an optional --sdk-path <path>".into(),
            )),
        },
        "build" => {
            let f = flags(&["--check", "--json"])?;
            Ok((
                Command::Build {
                    check: f.iter().any(|a| a == "--check"),
                },
                f.iter().any(|a| a == "--json"),
            ))
        }
        "test" => {
            let f = flags(&["--json"])?;
            Ok((Command::Test, !f.is_empty()))
        }
        "explain" => match rest {
            [code] => Ok((Command::Explain(code.clone()), false)),
            _ => Err(usage_error("explain takes one error code".into())),
        },
        other => Err(usage_error(format!("unknown command `{other}`"))),
    }
}

fn execute(cmd: &Command, report: &mut Report) -> Result<(), Diag> {
    let cargo = build::cargo_bin();
    let cwd = std::env::current_dir().map_err(|e| {
        Diag::new(
            "Z0209",
            format!("cannot read the current directory: {e}"),
            explain::fix("Z0209"),
        )
    })?;
    let ws = build::load_workspace(&cargo, &cwd)?;
    let pin = zeno_exec::pin();
    let exec = zeno_exec::resolve(&ws.root, &pin, &mut |note| eprintln!("{note}"))?;
    let version = zeno_exec::version(&exec)?;
    let strict = matches!(cmd, Command::Build { check: true });
    let (warnings, error) = zeno_exec::check_revision(&version, &pin, strict);
    report.warnings.extend(warnings);
    if let Some(e) = error {
        return Err(e);
    }
    match cmd {
        Command::Build { check: false } => build::build(&ws, &exec, report),
        Command::Build { check: true } => build::check(&ws, &cargo, &exec, report),
        Command::Test => test::run_tests(&ws, &exec, report),
        Command::New { .. } | Command::Explain(_) | Command::Help => {
            unreachable!("handled before execute")
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, json) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(d) => {
            eprintln!("{}\n{USAGE}", d.render("error"));
            return ExitCode::from(2);
        }
    };
    match &cmd {
        Command::Help => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Command::New { name, sdk_path } => {
            let cwd = std::env::current_dir().unwrap_or_default();
            return match new::new_project(
                &cwd,
                name,
                sdk_path.as_deref().map(std::path::Path::new),
                &build::cargo_bin(),
            ) {
                Ok(dir) => {
                    println!("created {}", dir.display());
                    println!("next: cd {name} && cargo zeno test");
                    ExitCode::SUCCESS
                }
                Err(d) => {
                    eprintln!("{}", d.render("error"));
                    ExitCode::from(1)
                }
            };
        }
        Command::Explain(code) => {
            return match explain::section(code) {
                Some(s) => {
                    println!("{s}");
                    ExitCode::SUCCESS
                }
                None => {
                    let d = Diag::new(
                        "Z0212",
                        format!("no section for `{code}`"),
                        explain::fix("Z0212"),
                    );
                    eprintln!("{}", d.render("error"));
                    ExitCode::from(1)
                }
            };
        }
        _ => {}
    }

    let mut report = Report::default();
    if let Err(d) = execute(&cmd, &mut report) {
        report.errors.push(d);
    }
    report.ok = report.errors.is_empty();

    for w in &report.warnings {
        eprintln!("{}", w.render("warning"));
    }
    for e in &report.errors {
        eprintln!("{}", e.render("error"));
    }
    if json {
        match serde_json::to_string(&report) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("cannot write the report: {e}"),
        }
    } else {
        for a in &report.artifacts {
            println!(
                "built {}: code_hash {}, {} bytes, {} initial page(s)",
                a.name, a.code_hash, a.size, a.initial_pages
            );
        }
        if let Some(t) = &report.tests {
            println!("tests: {} passed, {} failed", t.passed, t.failed.len());
        }
    }
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
