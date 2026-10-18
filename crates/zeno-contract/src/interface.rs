//! The interface file builder and writer. It runs on the host only,
//! from the test the `#[contract]` macro generates.

use std::string::{String, ToString};
use std::vec::Vec;
use std::{format, vec};

/// A named, typed field of a method, event, error or key.
pub struct Field {
    pub name: &'static str,
    pub ty: &'static str,
}

struct Method {
    name: &'static str,
    selector: [u8; 4],
    payable: bool,
    readonly: bool,
    inputs: Vec<(&'static str, &'static str)>,
    outputs: Vec<(&'static str, &'static str)>,
    description: Option<&'static str>,
}

struct EventDef {
    name: &'static str,
    topic: [u8; 32],
    fields: Vec<(&'static str, &'static str, bool)>,
    description: Option<&'static str>,
}

struct ErrorDef {
    code: i32,
    name: &'static str,
    fields: Vec<(&'static str, &'static str)>,
    description: &'static str,
}

struct StorageDef {
    name: &'static str,
    map: bool,
    namespace: u8,
    key: Vec<(&'static str, &'static str)>,
    hashed: bool,
    value: &'static str,
    description: Option<&'static str>,
}

/// The contract's interface, in declaration order.
#[derive(Default)]
pub struct Interface {
    methods: Vec<Method>,
    on_deposit: Option<&'static str>,
    events: Vec<EventDef>,
    errors: Vec<ErrorDef>,
    storage: Vec<StorageDef>,
}

fn pairs(fields: &[Field]) -> Vec<(&'static str, &'static str)> {
    fields.iter().map(|f| (f.name, f.ty)).collect()
}

impl Interface {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn method(
        &mut self,
        name: &'static str,
        selector: [u8; 4],
        payable: bool,
        readonly: bool,
        inputs: &[Field],
        output_names: &[&'static str],
        output_types: &[&'static str],
        description: Option<&'static str>,
    ) {
        self.methods.push(Method {
            name,
            selector,
            payable,
            readonly,
            inputs: pairs(inputs),
            outputs: output_names
                .iter()
                .copied()
                .zip(output_types.iter().copied())
                .collect(),
            description,
        });
    }

    pub fn on_deposit(&mut self, name: &'static str) {
        self.on_deposit = Some(name);
    }

    pub fn event(
        &mut self,
        name: &'static str,
        topic: [u8; 32],
        fields: &[(Field, bool)],
        description: Option<&'static str>,
    ) {
        self.events.push(EventDef {
            name,
            topic,
            fields: fields
                .iter()
                .map(|(f, indexed)| (f.name, f.ty, *indexed))
                .collect(),
            description,
        });
    }

    pub fn error(
        &mut self,
        code: i32,
        name: &'static str,
        fields: &[Field],
        description: &'static str,
    ) {
        self.errors.push(ErrorDef {
            code,
            name,
            fields: pairs(fields),
            description,
        });
    }

    pub fn item(
        &mut self,
        name: &'static str,
        namespace: u8,
        value: &'static str,
        description: Option<&'static str>,
    ) {
        self.storage.push(StorageDef {
            name,
            map: false,
            namespace,
            key: Vec::new(),
            hashed: false,
            value,
            description,
        });
    }

    pub fn map(
        &mut self,
        name: &'static str,
        namespace: u8,
        key: &[Field],
        hashed: bool,
        value: &'static str,
        description: Option<&'static str>,
    ) {
        self.storage.push(StorageDef {
            name,
            map: true,
            namespace,
            key: pairs(key),
            hashed,
            value,
            description,
        });
    }

    /// Checks that selectors, codes, namespaces and event names are
    /// distinct and that every type is one the interface format allows.
    pub fn check(&self) -> Result<(), String> {
        for (i, m) in self.methods.iter().enumerate() {
            if self.methods[..i].iter().any(|o| o.selector == m.selector) {
                return Err(format!(
                    "Z0108: duplicate selector {} on method `{}`\nfix: rename one method",
                    hex(&m.selector),
                    m.name
                ));
            }
        }
        for (i, e) in self.errors.iter().enumerate() {
            if self.errors[..i].iter().any(|o| o.code == e.code) {
                return Err(format!(
                    "Z0110: duplicate error code {} on `{}`\nfix: give each variant its own code",
                    e.code, e.name
                ));
            }
        }
        for (i, s) in self.storage.iter().enumerate() {
            if self.storage[..i].iter().any(|o| o.namespace == s.namespace) {
                return Err(format!(
                    "Z0117: duplicate namespace 0x{:02x} on `{}`\nfix: give each entry its own namespace",
                    s.namespace, s.name
                ));
            }
        }
        for (i, e) in self.events.iter().enumerate() {
            if self.events[..i].iter().any(|o| o.name == e.name) {
                return Err(format!(
                    "Z0115: duplicate event name `{}`\nfix: use a unique event name",
                    e.name
                ));
            }
        }
        let mut types: Vec<(&str, &str)> = Vec::new();
        for m in &self.methods {
            types.extend(m.inputs.iter().chain(&m.outputs).map(|(_, t)| (m.name, *t)));
        }
        for e in &self.events {
            types.extend(e.fields.iter().map(|(_, t, _)| (e.name, *t)));
        }
        for e in &self.errors {
            types.extend(e.fields.iter().map(|(_, t)| (e.name, *t)));
        }
        for s in &self.storage {
            types.extend(s.key.iter().map(|(_, t)| (s.name, *t)));
            types.push((s.name, s.value));
        }
        for (owner, ty) in types {
            if !known_type(ty) {
                return Err(format!(
                    "Z0122: unknown type `{ty}` on `{owner}`\nfix: use a type from the supported list"
                ));
            }
        }
        Ok(())
    }

    /// Renders the interface file for the module with `code_hash`.
    pub fn to_json(&self, code_hash: &str) -> String {
        let fields = |f: &[(&str, &str)]| {
            Json::Arr(
                f.iter()
                    .map(|(n, t)| Json::Obj(vec![("name", Json::str(n)), ("type", Json::str(t))]))
                    .collect(),
            )
        };
        let opt_description = |obj: &mut Vec<(&'static str, Json)>, d: Option<&str>| {
            if let Some(d) = d {
                obj.push(("description", Json::str(d)));
            }
        };

        let methods = self.methods.iter().map(|m| {
            let mut o = vec![
                ("name", Json::str(m.name)),
                ("selector", Json::Str(hex(&m.selector))),
                ("payable", Json::Bool(m.payable)),
                ("readonly", Json::Bool(m.readonly)),
                ("inputs", fields(&m.inputs)),
                ("outputs", fields(&m.outputs)),
            ];
            opt_description(&mut o, m.description);
            Json::Obj(o)
        });
        let on_deposit = match self.on_deposit {
            Some(name) => Json::Obj(vec![
                ("name", Json::str(name)),
                ("outputs", Json::Arr(Vec::new())),
            ]),
            None => Json::Null,
        };
        let events = self.events.iter().map(|e| {
            let event_fields = e
                .fields
                .iter()
                .map(|(n, t, indexed)| {
                    Json::Obj(vec![
                        ("name", Json::str(n)),
                        ("type", Json::str(t)),
                        ("indexed", Json::Bool(*indexed)),
                    ])
                })
                .collect();
            let mut o = vec![
                ("name", Json::str(e.name)),
                ("topic", Json::Str(hex(&e.topic))),
                ("fields", Json::Arr(event_fields)),
            ];
            opt_description(&mut o, e.description);
            Json::Obj(o)
        });
        let mut errors: Vec<Json> = self
            .errors
            .iter()
            .map(|e| error_json(e.code, e.name, fields(&e.fields), e.description))
            .collect();
        for (code, name, description) in SDK_ERRORS {
            if code == -3 && self.on_deposit.is_some() {
                continue;
            }
            errors.push(error_json(code, name, Json::Arr(Vec::new()), description));
        }
        let storage = self.storage.iter().map(|s| {
            let mut o = vec![
                ("name", Json::str(s.name)),
                ("kind", Json::str(if s.map { "map" } else { "item" })),
                ("namespace", Json::Str(format!("0x{:02x}", s.namespace))),
                ("key", fields(&s.key)),
                ("hashed", Json::Bool(s.hashed)),
                ("value", Json::str(s.value)),
            ];
            opt_description(&mut o, s.description);
            Json::Obj(o)
        });

        let doc = Json::Obj(vec![
            ("interface_version", Json::Int(1)),
            ("encoding", Json::str("zeno-selector-v1")),
            ("code_hash", Json::str(code_hash)),
            ("methods", Json::Arr(methods.collect())),
            ("on_deposit", on_deposit),
            ("events", Json::Arr(events.collect())),
            ("errors", Json::Arr(errors)),
            ("storage", Json::Arr(storage.collect())),
        ]);
        let mut out = String::new();
        doc.write(&mut out, 0);
        out.push('\n');
        out
    }
}

/// The SDK abort codes a module can raise, with their descriptions.
const SDK_ERRORS: [(i32, &str, &str); 8] = [
    (
        -1,
        "MalformedPayload",
        "a non-empty payload is shorter than a selector, or its arguments do not decode or do not fill the method's exact length",
    ),
    (
        -2,
        "UnknownSelector",
        "no method has the payload's selector",
    ),
    (
        -3,
        "NoDepositHandler",
        "an empty payload reached a contract with no deposit handler",
    ),
    (
        -4,
        "CapExceeded",
        "an item added to the effects would break a §25.1 cap, or does not fit the effects arena or the output buffer",
    ),
    (
        -5,
        "ClaimAboveDeposit",
        "claim_deposit(x) with x above the claimable amount",
    ),
    (-6, "StoredValueDecode", "a stored value fails to decode"),
    (-8, "Panic", "the module panicked"),
    (
        -9,
        "InvalidRecipient",
        "withdraw() to an L1 address that is not a user address",
    ),
];

fn error_json(code: i32, name: &str, fields: Json, description: &str) -> Json {
    Json::Obj(vec![
        ("code", Json::Int(i64::from(code))),
        ("name", Json::str(name)),
        ("fields", fields),
        ("description", Json::str(description)),
    ])
}

fn hex(b: &[u8]) -> String {
    let mut s = String::new();
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn known_type(ty: &str) -> bool {
    match ty {
        "u8" | "u16" | "u32" | "u64" | "u128" | "u256" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "bool" | "hash" | "l1Address" | "bytes" | "string" => true,
        _ => ty
            .strip_prefix("bytes")
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=32).contains(&n) && n.to_string() == ty["bytes".len()..]),
    }
}

enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

impl Json {
    fn str(s: &str) -> Json {
        Json::Str(s.to_string())
    }

    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(i) => out.push_str(&i.to_string()),
            Json::Str(s) => write_string(out, s),
            Json::Arr(items) if items.is_empty() => out.push_str("[]"),
            Json::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    indent(out, depth + 1);
                    item.write(out, depth + 1);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                indent(out, depth);
                out.push(']');
            }
            Json::Obj(members) => {
                out.push_str("{\n");
                for (i, (key, value)) in members.iter().enumerate() {
                    indent(out, depth + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, depth + 1);
                    out.push_str(if i + 1 < members.len() { ",\n" } else { "\n" });
                }
                indent(out, depth);
                out.push('}');
            }
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The body of the test `#[contract]` generates. It checks the
/// interface `build` returns. When `ZENO_INTERFACE_OUT` is set it also
/// writes the file there, with the code hash in `ZENO_CODE_HASH`.
pub fn interface_file_test(build: fn() -> Interface) {
    let interface = build();
    if let Err(message) = interface.check() {
        panic!("{message}");
    }
    let Some(path) = std::env::var_os("ZENO_INTERFACE_OUT") else {
        return;
    };
    let hash = std::env::var("ZENO_CODE_HASH").unwrap_or_default();
    let valid = hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !valid {
        panic!(
            "ZENO_CODE_HASH must be 64 lowercase hex characters when ZENO_INTERFACE_OUT is set\nfix: set ZENO_CODE_HASH to the code hash of the stripped module"
        );
    }
    if let Err(e) = std::fs::write(&path, interface.to_json(&hash)) {
        panic!("cannot write {}: {e}", path.to_string_lossy());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANT: &str = r#"{
  "interface_version": 1,
  "encoding": "zeno-selector-v1",
  "code_hash": "0000000000000000000000000000000000000000000000000000000000000000",
  "methods": [
    {
      "name": "send",
      "selector": "01020304",
      "payable": true,
      "readonly": false,
      "inputs": [
        {
          "name": "to",
          "type": "hash"
        }
      ],
      "outputs": [
        {
          "name": "ok",
          "type": "bool"
        }
      ],
      "description": "sends \"it\""
    }
  ],
  "on_deposit": null,
  "events": [
    {
      "name": "Sent",
      "topic": "abababababababababababababababababababababababababababababababab",
      "fields": [
        {
          "name": "to",
          "type": "hash",
          "indexed": true
        },
        {
          "name": "n",
          "type": "u64",
          "indexed": false
        }
      ]
    }
  ],
  "errors": [
    {
      "code": 1,
      "name": "Bad",
      "fields": [
        {
          "name": "x",
          "type": "u8"
        }
      ],
      "description": "it is bad"
    },
    {
      "code": -1,
      "name": "MalformedPayload",
      "fields": [],
      "description": "a non-empty payload is shorter than a selector, or its arguments do not decode or do not fill the method's exact length"
    },
    {
      "code": -2,
      "name": "UnknownSelector",
      "fields": [],
      "description": "no method has the payload's selector"
    },
    {
      "code": -3,
      "name": "NoDepositHandler",
      "fields": [],
      "description": "an empty payload reached a contract with no deposit handler"
    },
    {
      "code": -4,
      "name": "CapExceeded",
      "fields": [],
      "description": "an item added to the effects would break a §25.1 cap, or does not fit the effects arena or the output buffer"
    },
    {
      "code": -5,
      "name": "ClaimAboveDeposit",
      "fields": [],
      "description": "claim_deposit(x) with x above the claimable amount"
    },
    {
      "code": -6,
      "name": "StoredValueDecode",
      "fields": [],
      "description": "a stored value fails to decode"
    },
    {
      "code": -8,
      "name": "Panic",
      "fields": [],
      "description": "the module panicked"
    },
    {
      "code": -9,
      "name": "InvalidRecipient",
      "fields": [],
      "description": "withdraw() to an L1 address that is not a user address"
    }
  ],
  "storage": [
    {
      "name": "seen",
      "kind": "map",
      "namespace": "0x02",
      "key": [
        {
          "name": "who",
          "type": "hash"
        }
      ],
      "hashed": false,
      "value": "bool"
    }
  ]
}
"#;

    fn sample() -> Interface {
        let mut i = Interface::new();
        i.method(
            "send",
            [1, 2, 3, 4],
            true,
            false,
            &[Field {
                name: "to",
                ty: "hash",
            }],
            &["ok"],
            &["bool"],
            Some("sends \"it\""),
        );
        i.event(
            "Sent",
            [0xab; 32],
            &[
                (
                    Field {
                        name: "to",
                        ty: "hash",
                    },
                    true,
                ),
                (
                    Field {
                        name: "n",
                        ty: "u64",
                    },
                    false,
                ),
            ],
            None,
        );
        i.error(
            1,
            "Bad",
            &[Field {
                name: "x",
                ty: "u8",
            }],
            "it is bad",
        );
        i.map(
            "seen",
            0x02,
            &[Field {
                name: "who",
                ty: "hash",
            }],
            false,
            "bool",
            None,
        );
        i
    }

    #[test]
    fn json_text_is_exact() {
        let json = sample().to_json(&"0".repeat(64));
        assert_eq!(json, WANT);
    }

    #[test]
    fn check_accepts_a_valid_interface() {
        assert_eq!(sample().check(), Ok(()));
    }

    #[test]
    fn check_rejects_a_duplicate_selector() {
        let mut i = sample();
        i.method("other", [1, 2, 3, 4], false, false, &[], &[], &[], None);
        let message = i.check().unwrap_err();
        assert!(message.starts_with("Z0108: duplicate selector 01020304"));
        assert!(message.contains("\nfix: "));
    }

    #[test]
    fn check_rejects_an_unknown_type() {
        let mut i = sample();
        i.item("x", 0x03, "bytes33", None);
        assert!(i.check().unwrap_err().contains("bytes33"));
        let mut i = sample();
        i.item("x", 0x03, "bytes05", None);
        assert!(i.check().is_err());
    }
}
