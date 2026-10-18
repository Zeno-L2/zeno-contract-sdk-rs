//! Runs the built example modules in a `zeno-exec run` session and
//! checks the receipts. `ZENO_EXEC` is the path of the `zeno-exec`
//! binary and `ZENO_SESSION_DIR` the directory holding the stripped
//! `counter.wasm`, `counter-v2.wasm` and `vault-shape.wasm`. Each test
//! checks nothing when either is unset, unless
//! `ZENO_REQUIRE_WASM_TESTS` is 1, which makes it fail.

use sha3::{Digest, Sha3_256};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const SENDER: &str = "00a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const RECIPIENT: &str = "00c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3";

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: String,
}

/// The raw text after `"name":` up to the next `,` or `}`, without
/// quotes.
fn field(json: &str, name: &str) -> String {
    let key = format!("\"{name}\":");
    let at = json
        .find(&key)
        .unwrap_or_else(|| panic!("no {name} in {json}"));
    let rest = &json[at + key.len()..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim_matches('"').to_string()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn asset(n: u8) -> String {
    format!("{}{}", "00".repeat(22), format!("{n:02x}").repeat(10))
}

fn word(n: u64) -> String {
    format!("{n:064x}")
}

/// SHA3-256 of the parts, in hex.
fn sha3(parts: &[&[u8]]) -> String {
    let mut h = Sha3_256::new();
    for p in parts {
        h.update(p);
    }
    hex(&h.finalize())
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// The balance leaf key of the vault: SHA3-256(0x01 || caller ||
/// asset).
fn balance_key(sender: &str, asset: &str) -> String {
    let caller = sha3(&[b"L1_CALLER:", &unhex(sender)]);
    sha3(&[&[0x01], &unhex(&caller), &unhex(asset)])
}

impl Session {
    fn start(exec: &str, module: &str) -> Session {
        let mut child = Command::new(exec)
            .arg("run")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start zeno-exec");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut s = Session {
            child,
            stdin,
            stdout,
            id: String::new(),
        };
        let wasm = std::fs::read(module).unwrap_or_else(|e| panic!("{module}: {e}"));
        let deployed = s.send(&format!(
            "{{\"op\":\"deploy\",\"sender\":\"{SENDER}\",\"module\":\"{}\",\"metadata\":\"immutable\"}}",
            hex(&wasm)
        ));
        assert_eq!(field(&deployed, "status"), "SUCCESS", "{deployed}");
        s.id = field(&deployed, "contractId");
        let bought = s.send(&format!(
            "{{\"op\":\"buyStorage\",\"sender\":\"{SENDER}\",\"contractId\":\"{}\",\"bytes\":16384}}",
            s.id
        ));
        assert_eq!(field(&bought, "status"), "SUCCESS", "{bought}");
        s
    }

    fn send(&mut self, line: &str) -> String {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
        let mut out = String::new();
        self.stdout.read_line(&mut out).unwrap();
        out
    }

    /// Sends a call and returns `(status, abortCode, returnData)`.
    fn call(&mut self, payload: &str, deposit: Option<(String, u64)>) -> (String, i32, String) {
        let deposit = match deposit {
            Some((a, n)) => format!(",\"deposit\":{{\"asset\":\"{a}\",\"amount\":\"{n}\"}}"),
            None => String::new(),
        };
        let r = self.send(&format!(
            "{{\"op\":\"call\",\"sender\":\"{SENDER}\",\"contractId\":\"{}\",\"payload\":\"{payload}\"{deposit}}}",
            self.id
        ));
        (
            field(&r, "status"),
            field(&r, "abortCode").parse().unwrap(),
            field(&r, "returnData"),
        )
    }

    fn get(&mut self, key: &str) -> String {
        let r = self.send(&format!(
            "{{\"op\":\"getStorage\",\"contractId\":\"{}\",\"localKey\":\"{key}\"}}",
            self.id
        ));
        field(&r, "value")
    }

    fn set(&mut self, key: &str, value: &str) {
        let r = self.send(&format!(
            "{{\"op\":\"setStorage\",\"contractId\":\"{}\",\"localKey\":\"{key}\",\"value\":\"{value}\"}}",
            self.id
        ));
        assert_eq!(r.trim(), "{\"ok\":true}");
    }

    fn finish(mut self) {
        drop(self.stdin);
        self.child.wait().unwrap();
    }
}

fn setup(module: &str) -> Option<Session> {
    let (Ok(exec), Ok(dir)) = (
        std::env::var("ZENO_EXEC"),
        std::env::var("ZENO_SESSION_DIR"),
    ) else {
        if std::env::var("ZENO_REQUIRE_WASM_TESTS").as_deref() == Ok("1") {
            panic!(
                "ZENO_EXEC or ZENO_SESSION_DIR is unset and ZENO_REQUIRE_WASM_TESTS is 1\nfix: build zeno-exec and the stripped modules, then set both variables"
            );
        }
        println!("ZENO_EXEC or ZENO_SESSION_DIR unset, no session run");
        return None;
    };
    Some(Session::start(&exec, &format!("{dir}/{module}.wasm")))
}

fn revert(code: i32) -> (String, i32, String) {
    ("REVERT".to_string(), code, String::new())
}

#[test]
fn counter_v2_routes_and_counts() {
    let Some(mut s) = setup("counter-v2") else {
        return;
    };
    let ok = |n: u64| ("SUCCESS".to_string(), 0, format!("{n:016x}"));
    assert_eq!(s.call("5164a106", None), ok(1));
    assert_eq!(s.call("5164a106", None), ok(2));
    assert_eq!(s.call("f7a1de2b", None), ok(1));
    assert_eq!(s.call("f7a1de2b", None), ok(0));
    assert_eq!(s.call("f7a1de2b", None).1, 2, "underflow");
    assert_eq!(s.call("", None), revert(-3), "empty payload, no handler");
    assert_eq!(s.call("5164a1", None), revert(-1), "short selector");
    assert_eq!(s.call("deadbeef", None), revert(-2), "unknown selector");
    assert_eq!(s.call("5164a10600", None), revert(-1), "leftover byte");
    s.finish();
}

#[test]
fn vault_shape_decodes_frame_and_arguments() {
    let Some(mut s) = setup("vault-shape") else {
        return;
    };
    let a = asset(1);
    let key = balance_key(SENDER, &a);

    let deposited = s.send(&format!(
        "{{\"op\":\"deposit\",\"sender\":\"{SENDER}\",\"contractId\":\"{}\",\"asset\":\"{a}\",\"amount\":\"1000\"}}",
        s.id
    ));
    assert_eq!(field(&deposited, "status"), "SUCCESS", "{deposited}");
    assert_eq!(field(&deposited, "claimedDeposit"), "1000");
    assert_eq!(s.get(&key), word(1000), "balance keyed by caller and asset");

    let (status, _, _) = s.call("", Some((a.clone(), 500)));
    assert_eq!(status, "SUCCESS", "payable empty call");
    assert_eq!(s.get(&key), word(1500));

    let withdraw = |asset: &str, amount: u64, recipient: &str| {
        format!("24b2e5fc{asset}{}{recipient}", word(amount))
    };
    assert_eq!(s.call(&withdraw(&a, 400, RECIPIENT), None).0, "SUCCESS");
    assert_eq!(s.get(&key), word(1100));
    assert_eq!(
        s.call(&withdraw(&a, 5000, RECIPIENT), None).1,
        1,
        "insufficient"
    );
    assert_eq!(
        s.call(&withdraw(&a, 0, RECIPIENT), None).1,
        3,
        "zero amount"
    );
    let non_user = format!("01{}", "c3".repeat(19));
    assert_eq!(s.call(&withdraw(&a, 1, &non_user), None).1, 4, "recipient");

    let bad_asset = format!("01{}{}", "00".repeat(21), "01".repeat(10));
    assert_eq!(
        s.call(&withdraw(&bad_asset, 1, RECIPIENT), None),
        revert(-1)
    );
    let ok = withdraw(&a, 1, RECIPIENT);
    assert_eq!(
        s.call(&ok[..ok.len() - 2], None),
        revert(-1),
        "short argument"
    );
    assert_eq!(
        s.call(&format!("{ok}00"), None),
        revert(-1),
        "leftover byte"
    );
    assert_eq!(s.call("2c99", None), revert(-1), "short selector");
    assert_eq!(s.call("deadbeef", None), revert(-2), "unknown selector");

    let echo = "2c99ba7400000003616263000000026869".to_string();
    assert_eq!(s.call(&echo, None).0, "SUCCESS", "borrowed arguments");
    assert_eq!(
        s.call(&format!("{echo}00"), None),
        revert(-1),
        "echo leftover"
    );
    assert_eq!(
        s.call(&echo[..echo.len() - 2], None),
        revert(-1),
        "echo short"
    );
    let bad_utf8 = "2c99ba74000000036162630000000200ff".to_string();
    assert_eq!(s.call(&bad_utf8, None), revert(-1), "invalid utf-8");

    s.set(&key, "0102030405");
    assert_eq!(
        s.call(&withdraw(&a, 1, RECIPIENT), None),
        revert(-6),
        "stored width"
    );
    s.finish();
}
