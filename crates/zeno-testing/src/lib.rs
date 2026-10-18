//! A test harness for Zeno contracts: each `TestDomain` drives one
//! `zeno-exec run` session.

mod receipt;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use sha3::{Digest, Sha3_256};

pub use receipt::{Event, Outbox, Receipt, StateWrite, Status};
pub use zeno_contract::{AssetId, Caller, ContractId, L1Address, U256, codes};
use zeno_contract::{Codec, Writer};

/// The fix line of each harness code.
fn fix_of(code: &str) -> &'static str {
    match code {
        "Z0401" => "run tests with `cargo zeno test`",
        "Z0402" => {
            "run contract tests with `cargo zeno test`; run host tests with `cargo test --workspace --exclude <contract crates>`"
        }
        "Z0403" => "use a listed method, or `call_raw`",
        "Z0404" => "read the stderr lines in the message",
        "Z0405" => "list the artifact in `[package.metadata.zeno]` and run `cargo zeno test`",
        "Z0406" => "the message names the op and the reason",
        "Z0407" => "the status and abort code in the message",
        _ => "",
    }
}

pub(crate) fn fail(code: &str, message: String) -> ! {
    panic!("{code}: {message}\nfix: {}", fix_of(code))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decodes lowercase hex; an empty string is valid.
pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    s.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            };
            Some(digit(pair[0])? << 4 | digit(pair[1])?)
        })
        .collect()
}

/// Writes `v` in decimal by repeated division of its bytes by 10.
fn decimal(v: U256) -> String {
    let mut bytes = v.to_be_bytes();
    let mut digits = Vec::new();
    while bytes.iter().any(|&b| b != 0) {
        let mut rem = 0u16;
        for b in bytes.iter_mut() {
            let cur = rem * 256 + u16::from(*b);
            *b = (cur / 10) as u8;
            rem = cur % 10;
        }
        digits.push(b'0' + rem as u8);
    }
    if digits.is_empty() {
        digits.push(b'0');
    }
    digits.reverse();
    String::from_utf8(digits).expect("ascii digits")
}

/// Reads a decimal string below 2^256.
pub(crate) fn parse_decimal(s: &str) -> Option<U256> {
    if s.is_empty() {
        return None;
    }
    let ten = U256::from(10u64);
    let mut acc = U256::ZERO;
    for c in s.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        acc = acc
            .checked_mul(ten)?
            .checked_add(U256::from(u64::from(c - b'0')))?;
    }
    Some(acc)
}

/// SHA3-256 of the concatenated parts.
pub fn sha3_256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha3_256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// The first four bytes of SHA3-256 of the method name.
pub fn selector(method: &str) -> [u8; 4] {
    let h = sha3_256(&[method.as_bytes()]);
    [h[0], h[1], h[2], h[3]]
}

/// A method's arguments: `()` or a tuple of up to eight values.
pub trait Encode {
    fn encode(&self, w: &mut Writer<'_>);
}

impl Encode for () {
    fn encode(&self, _: &mut Writer<'_>) {}
}

macro_rules! encode_tuples {
    ($(($($t:ident $i:tt),+);)*) => {$(
        impl<'x, $($t: Codec<'x>),+> Encode for ($($t,)+) {
            fn encode(&self, w: &mut Writer<'_>) {
                $(Codec::encode(&self.$i, w);)+
            }
        }
    )*};
}
encode_tuples! {
    (A 0);
    (A 0, B 1);
    (A 0, B 1, C 2);
    (A 0, B 1, C 2, D 3);
    (A 0, B 1, C 2, D 3, E 4);
    (A 0, B 1, C 2, D 3, E 4, F 5);
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6);
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    address: L1Address,
}

impl Account {
    pub fn address(&self) -> L1Address {
        self.address
    }

    /// The caller hash a contract sees:
    /// SHA3-256("L1_CALLER:" || address).
    pub fn caller(&self) -> Caller {
        Caller::new(sha3_256(&[b"L1_CALLER:", &self.address.to_bytes()]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contract {
    id: ContractId,
}

impl Contract {
    pub fn id(&self) -> ContractId {
        self.id
    }

    /// The state path of a contract-local key.
    pub fn leaf_path(&self, local_key: &[u8]) -> [u8; 32] {
        sha3_256(&[&self.id.to_bytes(), local_key])
    }
}

/// The methods of a deployed interface, by name.
struct Interface {
    methods: Vec<(String, [u8; 4])>,
}

pub struct TestDomain {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// Drains the child's stderr as it runs and returns its last lines.
    stderr: Option<std::thread::JoinHandle<Vec<String>>>,
    dir: PathBuf,
    interfaces: HashMap<[u8; 32], Interface>,
}

impl Default for TestDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl TestDomain {
    pub fn new() -> TestDomain {
        let var = |k: &str| std::env::var(k).ok();
        TestDomain::start(
            var("ZENO_FRESH"),
            var("ZENO_EXEC"),
            var("CARGO_MANIFEST_DIR"),
        )
    }

    fn start(fresh: Option<String>, exec: Option<String>, dir: Option<String>) -> TestDomain {
        if fresh.as_deref() != Some("1") {
            fail(
                "Z0402",
                "the contract artifacts may be stale; this test was not started by `cargo zeno test`"
                    .into(),
            );
        }
        let Some(exec) = exec.filter(|e| !e.is_empty()) else {
            fail("Z0401", "ZENO_EXEC is not set".into());
        };
        let mut child = Command::new(&exec)
            .arg("run")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| fail("Z0401", format!("`{exec} run` did not start: {e}")));
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let stderr = child.stderr.take().map(|e| {
            std::thread::spawn(move || {
                let mut tail = std::collections::VecDeque::new();
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    if tail.len() == 20 {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
                tail.into()
            })
        });
        TestDomain {
            child,
            stdin,
            stdout,
            stderr,
            dir: PathBuf::from(dir.unwrap_or_else(|| ".".into())),
            interfaces: HashMap::new(),
        }
    }

    /// The account with address 0x00 followed by 19 bytes of `n`.
    pub fn account(&self, n: u8) -> Account {
        let mut a = [n; 20];
        a[0] = 0;
        Account {
            address: L1Address::new(a),
        }
    }

    /// Deploys `artifact` with `from` as its owner.
    pub fn deploy(&mut self, artifact: &str, from: &Account) -> Contract {
        self.deploy_with(artifact, from, owner_metadata(from))
    }

    /// Deploys `artifact` as immutable.
    pub fn deploy_immutable(&mut self, artifact: &str, from: &Account) -> Contract {
        self.deploy_with(artifact, from, json!("immutable"))
    }

    /// A handle for a contract id. It registers no interface, so `call`
    /// works only on ids that `deploy` produced.
    pub fn contract_at(&self, id: ContractId) -> Contract {
        Contract { id }
    }

    pub fn call(
        &mut self,
        c: &Contract,
        from: &Account,
        method: &str,
        args: impl Encode,
    ) -> Receipt {
        let payload = self.payload(c, method, &args);
        self.call_raw(c, from, &payload, None)
    }

    pub fn call_with_deposit(
        &mut self,
        c: &Contract,
        from: &Account,
        method: &str,
        args: impl Encode,
        asset: AssetId,
        amount: U256,
    ) -> Receipt {
        let payload = self.payload(c, method, &args);
        self.call_raw(c, from, &payload, Some((asset, amount)))
    }

    /// An empty payload with a deposit reaches `#[on_deposit]`.
    pub fn call_raw(
        &mut self,
        c: &Contract,
        from: &Account,
        payload: &[u8],
        deposit: Option<(AssetId, U256)>,
    ) -> Receipt {
        let mut cmd = json!({
            "op": "call",
            "sender": address_hex(from),
            "contractId": hex(&c.id.to_bytes()),
            "payload": hex(payload),
        });
        if let Some((asset, amount)) = deposit {
            cmd["deposit"] = json!({ "asset": hex(&asset.to_bytes()), "amount": decimal(amount) });
        }
        let res = self.send(cmd);
        Receipt::from_json(&res["receipt"])
    }

    pub fn deposit(
        &mut self,
        c: &Contract,
        from: &Account,
        asset: AssetId,
        amount: U256,
    ) -> Receipt {
        let res = self.send(json!({
            "op": "deposit",
            "sender": address_hex(from),
            "contractId": hex(&c.id.to_bytes()),
            "asset": hex(&asset.to_bytes()),
            "amount": decimal(amount),
        }));
        Receipt::from_json(&res["receipt"])
    }

    /// Upgrades `c` to `artifact` with `from` as the owner. On success
    /// the contract's interface becomes the artifact's.
    pub fn upgrade(&mut self, c: &Contract, from: &Account, artifact: &str) -> Receipt {
        let (module, interface) = self.artifact(artifact);
        let res = self.send(json!({
            "op": "upgrade",
            "sender": address_hex(from),
            "contractId": hex(&c.id.to_bytes()),
            "module": hex(&module),
            "metadata": owner_metadata(from),
        }));
        let receipt = Receipt::from_json(&res["receipt"]);
        if receipt.status == Status::Success {
            self.interfaces.insert(c.id.to_bytes(), interface);
        }
        receipt
    }

    pub fn transfer_ownership(&mut self, c: &Contract, from: &Account, to: &Account) -> Receipt {
        let res = self.send(json!({
            "op": "transferOwnership",
            "sender": address_hex(from),
            "contractId": hex(&c.id.to_bytes()),
            "newOwner": address_hex(to),
        }));
        Receipt::from_json(&res["receipt"])
    }

    pub fn buy_storage(&mut self, c: &Contract, from: &Account, bytes: u64) -> Receipt {
        let res = self.send(json!({
            "op": "buyStorage",
            "sender": address_hex(from),
            "contractId": hex(&c.id.to_bytes()),
            "bytes": bytes,
        }));
        Receipt::from_json(&res["receipt"])
    }

    /// Writes a contract-local key directly; `None` deletes it.
    pub fn set_storage(&mut self, c: &Contract, local_key: &[u8], value: Option<&[u8]>) {
        self.send(json!({
            "op": "setStorage",
            "contractId": hex(&c.id.to_bytes()),
            "localKey": hex(local_key),
            "value": value.map(hex),
        }));
    }

    /// Reads a contract-local key.
    pub fn storage(&mut self, c: &Contract, local_key: &[u8]) -> Option<Vec<u8>> {
        let res = self.send(json!({
            "op": "getStorage",
            "contractId": hex(&c.id.to_bytes()),
            "localKey": hex(local_key),
        }));
        res["value"].as_str().map(|s| {
            unhex(s)
                .unwrap_or_else(|| fail("Z0404", "getStorage wrote a value that is not hex".into()))
        })
    }

    fn deploy_with(&mut self, artifact: &str, from: &Account, metadata: Value) -> Contract {
        let (module, interface) = self.artifact(artifact);
        let res = self.send(json!({
            "op": "deploy",
            "sender": address_hex(from),
            "module": hex(&module),
            "metadata": metadata,
        }));
        let receipt = Receipt::from_json(&res["receipt"]);
        if receipt.status != Status::Success {
            fail(
                "Z0407",
                format!(
                    "deploy of `{artifact}` returned status {:?}, abort code {}",
                    receipt.status, receipt.abort_code
                ),
            );
        }
        let id: [u8; 32] = res["contractId"]
            .as_str()
            .and_then(unhex)
            .and_then(|b| b.try_into().ok())
            .unwrap_or_else(|| fail("Z0404", "deploy wrote no contractId".into()));
        self.interfaces.insert(id, interface);
        Contract {
            id: ContractId::new(id),
        }
    }

    /// Reads an artifact's module and interface from the crate's
    /// `artifacts` directory.
    fn artifact(&self, name: &str) -> (Vec<u8>, Interface) {
        let read = |ext: &str| {
            let path = self.dir.join("artifacts").join(format!("{name}.{ext}"));
            std::fs::read(&path)
                .unwrap_or_else(|e| fail("Z0405", format!("cannot read {}: {e}", path.display())))
        };
        let module = read("wasm");
        let doc = read("interface.json");
        let v: Value = serde_json::from_slice(&doc)
            .unwrap_or_else(|e| fail("Z0405", format!("{name}.interface.json is not JSON: {e}")));
        let methods = v["methods"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|m| {
                        let name = m["name"].as_str()?.to_string();
                        let sel = unhex(m["selector"].as_str()?)?.try_into().ok()?;
                        Some((name, sel))
                    })
                    .collect()
            })
            .unwrap_or_default();
        (module, Interface { methods })
    }

    /// The selector of `method` followed by the encoded arguments.
    fn payload(&self, c: &Contract, method: &str, args: &impl Encode) -> Vec<u8> {
        let Some(interface) = self.interfaces.get(&c.id.to_bytes()) else {
            fail(
                "Z0403",
                format!(
                    "no interface is stored for contract {}; deploy it with this domain",
                    hex(&c.id.to_bytes())
                ),
            );
        };
        let Some((_, sel)) = interface.methods.iter().find(|(n, _)| n == method) else {
            let names: Vec<&str> = interface.methods.iter().map(|(n, _)| n.as_str()).collect();
            fail(
                "Z0403",
                format!(
                    "no method `{method}`; the interface has {}",
                    names.join(", ")
                ),
            );
        };
        let mut buf = [0u8; 8192];
        let mut w = Writer::new(&mut buf);
        args.encode(&mut w);
        assert!(
            !w.overflowed(),
            "the arguments of `{method}` exceed 8192 bytes"
        );
        let n = w.len();
        let mut payload = sel.to_vec();
        payload.extend_from_slice(&buf[..n]);
        payload
    }

    /// Writes one command line and reads its result line.
    fn send(&mut self, cmd: Value) -> Value {
        let op = cmd["op"].as_str().unwrap_or_default().to_string();
        let line = cmd.to_string();
        let stdin = self.stdin.as_mut().expect("stdin is open until drop");
        if writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .is_err()
        {
            self.died();
        }
        let mut out = String::new();
        match self.stdout.read_line(&mut out) {
            Ok(0) | Err(_) => self.died(),
            Ok(_) => {}
        }
        let v: Value = serde_json::from_str(out.trim_end()).unwrap_or_else(|_| {
            let shown: String = out.chars().take(200).collect();
            fail(
                "Z0404",
                format!("zeno-exec wrote a line that is not JSON: {shown}"),
            )
        });
        if v["ok"] != json!(true) {
            let kind = v["error"]["kind"].as_str().unwrap_or("unknown");
            let message = v["error"]["message"].as_str().unwrap_or("");
            fail("Z0406", format!("op `{op}` refused ({kind}): {message}"));
        }
        v
    }

    /// Reports a child that stopped, with the end of its stderr.
    fn died(&mut self) -> ! {
        self.stdin = None;
        let status = self.child.wait();
        let tail = self
            .stderr
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default()
            .join("\n");
        fail(
            "Z0404",
            format!("zeno-exec exited ({status:?}); its stderr ends:\n{tail}"),
        )
    }
}

impl Drop for TestDomain {
    fn drop(&mut self) {
        self.stdin = None;
        let _ = self.child.wait();
    }
}

fn address_hex(a: &Account) -> String {
    hex(&a.address.to_bytes())
}

fn owner_metadata(a: &Account) -> Value {
    json!({ "owner": address_hex(a) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_round_trip() {
        for v in [
            U256::ZERO,
            U256::from(1u64),
            U256::from(u64::MAX),
            U256::MAX,
        ] {
            assert_eq!(parse_decimal(&decimal(v)), Some(v));
        }
        assert_eq!(decimal(U256::ZERO), "0");
        assert_eq!(decimal(U256::from(u64::MAX)), "18446744073709551615");
        assert_eq!(
            decimal(U256::MAX),
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
        assert_eq!(parse_decimal(""), None);
        assert_eq!(parse_decimal("12a"), None);
        assert_eq!(
            parse_decimal(
                "115792089237316195423570985008687907853269984665640564039457584007913129639936"
            ),
            None
        );
    }

    fn encoded(args: impl Encode) -> Vec<u8> {
        let mut buf = [0u8; 256];
        let mut w = Writer::new(&mut buf);
        args.encode(&mut w);
        let n = w.len();
        buf[..n].to_vec()
    }

    #[test]
    fn arguments_encode_in_order() {
        assert!(encoded(()).is_empty());
        assert_eq!(encoded((5u64,)), [0, 0, 0, 0, 0, 0, 0, 5]);
        let mut want = U256::from(1u64).to_be_bytes().to_vec();
        want.push(1);
        want.extend([0, 0, 0, 2, b'a', b'b']);
        assert_eq!(encoded((U256::from(1u64), true, &b"ab"[..])), want);
    }

    #[test]
    fn selector_is_the_name_hash_prefix() {
        assert_eq!(hex(&selector("increment")), "5164a106");
    }

    #[test]
    #[should_panic(expected = "Z0402")]
    fn a_plain_cargo_test_run_is_refused() {
        TestDomain::start(None, None, None);
    }

    #[test]
    #[should_panic(expected = "Z0402")]
    fn a_fresh_flag_other_than_one_is_refused() {
        TestDomain::start(Some("0".into()), None, None);
    }

    #[test]
    #[should_panic(expected = "Z0402")]
    fn an_empty_fresh_flag_is_refused() {
        TestDomain::start(Some(String::new()), None, None);
    }

    #[test]
    fn a_child_with_a_large_stderr_is_reported_without_hanging() {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script =
            std::env::temp_dir().join(format!("zeno-testing-noisy-{}-{nanos}", std::process::id()));
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(&script)
                .unwrap();
            f.write_all(b"#!/bin/sh\nyes noise | head -n 30000 >&2\n")
                .unwrap();
        }
        let exec = script.to_string_lossy().into_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let mut d = TestDomain::start(Some("1".into()), Some(exec), None);
                let c = d.contract_at(ContractId::new([0; 32]));
                d.storage(&c, &[1]);
            });
            let _ = tx.send(result);
        });
        let outcome = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the harness hung on a large stderr");
        worker.join().unwrap();
        std::fs::remove_file(&script).unwrap();
        let payload = outcome.expect_err("the dead child must panic");
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(message.starts_with("Z0404"), "{message}");
        assert!(message.contains("noise"), "{message}");
    }

    #[test]
    fn leaf_path_hashes_the_id_and_key() {
        let c = Contract {
            id: ContractId::new([7; 32]),
        };
        assert_eq!(c.leaf_path(&[1]), sha3_256(&[&[7; 32], &[1]]));
    }

    #[test]
    fn accounts_start_with_a_zero_byte() {
        let d = Account {
            address: L1Address::new([0; 20]),
        };
        assert_ne!(d.caller(), Caller::new([0; 32]));
        let mut a = [0xa1; 20];
        a[0] = 0;
        assert_eq!(unhex(&hex(&a)).unwrap(), a);
    }
}
