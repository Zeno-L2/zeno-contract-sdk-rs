//! Receipts as `zeno-exec run` reports them, and the asserts over them.

use serde_json::Value;

use zeno_contract::{AssetId, ContractError, L1Address, Outputs, Reader, U256, Writer};

use crate::{fail, hex, parse_decimal, unhex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Success,
    Revert,
    OutOfGas,
    RuntimeFault,
    ValidationFailed,
    ResultTooLarge,
    Unauthorized,
    UnsupportedOperation,
    StorageQuotaExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub status: Status,
    pub gas_used: u64,
    pub abort_code: i32,
    pub return_data: Vec<u8>,
    pub claimed_deposit: U256,
    pub events: Vec<Event>,
    pub outbox: Vec<Outbox>,
    pub state_diff: Vec<StateWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outbox {
    pub source_contract: [u8; 32],
    pub recipient: L1Address,
    pub asset: AssetId,
    pub amount: U256,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateWrite {
    pub path: [u8; 32],
    pub value: Option<Vec<u8>>,
}

fn bad(what: &str) -> ! {
    fail("Z0404", format!("a receipt member is not valid: {what}"))
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_else(|| bad(key))
}

fn bytes_of(v: &Value, key: &str) -> Vec<u8> {
    unhex(text(v, key)).unwrap_or_else(|| bad(key))
}

fn array_of<const N: usize>(s: &str, what: &str) -> [u8; N] {
    unhex(s)
        .and_then(|b| b.try_into().ok())
        .unwrap_or_else(|| bad(what))
}

fn status_of(s: &str) -> Status {
    match s {
        "SUCCESS" => Status::Success,
        "REVERT" => Status::Revert,
        "OUT_OF_GAS" => Status::OutOfGas,
        "RUNTIME_FAULT" => Status::RuntimeFault,
        "VALIDATION_FAILED" => Status::ValidationFailed,
        "RESULT_TOO_LARGE" => Status::ResultTooLarge,
        "UNAUTHORIZED" => Status::Unauthorized,
        "UNSUPPORTED_OPERATION" => Status::UnsupportedOperation,
        "STORAGE_QUOTA_EXCEEDED" => Status::StorageQuotaExceeded,
        other => fail("Z0404", format!("unknown receipt status `{other}`")),
    }
}

fn recipient_of(s: &str) -> L1Address {
    match bech32::decode(s) {
        Ok((hrp, data)) if hrp.as_str() == "z" && data.len() == 20 => {
            L1Address::new(data.try_into().expect("20 bytes"))
        }
        _ => bad("recipient"),
    }
}

fn asset_of(s: &str) -> AssetId {
    let b: [u8; 32] = array_of(s, "assetId");
    if b[..22].iter().any(|&x| x != 0) {
        bad("assetId");
    }
    AssetId::from_zts(b[22..].try_into().expect("10 bytes"))
}

impl Receipt {
    /// Reads the `receipt` member of a command result.
    pub(crate) fn from_json(v: &Value) -> Receipt {
        let list = |key: &str| -> &[Value] {
            v[key]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| bad(key))
        };
        let events = list("events")
            .iter()
            .map(|e| Event {
                topics: e["topics"]
                    .as_array()
                    .unwrap_or_else(|| bad("topics"))
                    .iter()
                    .map(|t| array_of(t.as_str().unwrap_or_else(|| bad("topics")), "topics"))
                    .collect(),
                data: bytes_of(e, "data"),
            })
            .collect();
        let outbox = list("outbox")
            .iter()
            .map(|o| {
                if text(o, "kind") != "L1_WITHDRAWAL" {
                    fail(
                        "Z0404",
                        format!("unexpected outbox kind `{}`", text(o, "kind")),
                    );
                }
                Outbox {
                    source_contract: array_of(text(o, "sourceContract"), "sourceContract"),
                    recipient: recipient_of(text(o, "recipient")),
                    asset: asset_of(text(o, "assetId")),
                    amount: parse_decimal(text(o, "amount")).unwrap_or_else(|| bad("amount")),
                }
            })
            .collect();
        let state_diff = list("stateDiff")
            .iter()
            .map(|d| StateWrite {
                path: array_of(text(d, "path"), "path"),
                value: if d["value"].is_null() {
                    None
                } else {
                    Some(bytes_of(d, "value"))
                },
            })
            .collect();
        Receipt {
            status: status_of(text(v, "status")),
            gas_used: v["gasUsed"].as_u64().unwrap_or_else(|| bad("gasUsed")),
            abort_code: v["abortCode"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .unwrap_or_else(|| bad("abortCode")),
            return_data: bytes_of(v, "returnData"),
            claimed_deposit: parse_decimal(text(v, "claimedDeposit"))
                .unwrap_or_else(|| bad("claimedDeposit")),
            events,
            outbox,
            state_diff,
        }
    }

    fn describe(&self) -> String {
        format!(
            "status {:?}, abort code {}, return data {}",
            self.status,
            self.abort_code,
            hex(&self.return_data)
        )
    }

    pub fn assert_success(&self) -> &Self {
        assert!(
            self.status == Status::Success,
            "expected status Success, got {}",
            self.describe()
        );
        self
    }

    /// Requires a revert with the error's code and encoded fields, and
    /// no state changes or events.
    pub fn assert_error<E: ContractError>(&self, e: &E) -> &Self {
        let mut buf = [0u8; 4096];
        let mut w = Writer::new(&mut buf);
        e.encode_fields(&mut w);
        assert!(!w.overflowed(), "the error fields exceed 4096 bytes");
        let n = w.len();
        let fields = buf[..n].to_vec();
        assert!(
            self.status == Status::Revert
                && self.abort_code == e.code()
                && self.return_data == fields,
            "expected status Revert, abort code {}, return data {}; got {}",
            e.code(),
            hex(&fields),
            self.describe()
        );
        self.assert_no_effects()
    }

    /// Requires a revert with `code`, empty return data and no state
    /// changes or events.
    pub fn assert_abort(&self, code: i32) -> &Self {
        assert!(
            self.status == Status::Revert && self.abort_code == code && self.return_data.is_empty(),
            "expected status Revert, abort code {code}, empty return data; got {}",
            self.describe()
        );
        self.assert_no_effects()
    }

    fn assert_no_effects(&self) -> &Self {
        assert!(
            self.state_diff.is_empty() && self.events.is_empty(),
            "a reverted call left {} state writes and {} events",
            self.state_diff.len(),
            self.events.len()
        );
        self
    }

    /// Decodes the whole return data as `T`.
    pub fn decode_return<T: Outputs>(&self) -> T {
        let mut r = Reader::new(&self.return_data);
        match T::decode(&mut r) {
            Some(v) if r.is_empty() => v,
            _ => panic!(
                "return data {} does not decode as {} output(s)",
                hex(&self.return_data),
                T::COUNT
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bech32::Hrp;

    struct Boom;
    impl ContractError for Boom {
        fn code(&self) -> i32 {
            3
        }
        fn encode_fields(&self, w: &mut Writer<'_>) {
            w.raw(&[9, 9]);
        }
    }

    fn receipt(status: Status, abort_code: i32, return_data: &[u8]) -> Receipt {
        Receipt {
            status,
            gas_used: 0,
            abort_code,
            return_data: return_data.to_vec(),
            claimed_deposit: U256::ZERO,
            events: vec![],
            outbox: vec![],
            state_diff: vec![],
        }
    }

    #[test]
    fn a_receipt_line_is_parsed() {
        let recipient = bech32::encode::<bech32::Bech32>(Hrp::parse("z").unwrap(), &[0x11; 20])
            .expect("encodes");
        let id = "ab".repeat(32);
        let asset = format!("{}{}", "00".repeat(22), "01".repeat(10));
        let line = format!(
            r#"{{"status":"SUCCESS","gasUsed":42,"abortCode":0,"contractId":"{id}","codeHash":"{id}","returnData":"0000000000000002","events":[{{"contractId":"{id}","inputIndex":5,"eventIndex":0,"topics":["{id}"],"data":"dead"}}],"outbox":[{{"sourceContract":"{id}","inputIndex":5,"outboxIndex":0,"kind":"L1_WITHDRAWAL","targetContract":null,"recipient":"{recipient}","assetId":"{asset}","amount":"7","payload":"","outboxId":null}}],"stateDiff":[{{"path":"{id}","value":null}},{{"path":"{id}","value":"0102"}}],"claimedDeposit":"400"}}"#
        );
        let r = Receipt::from_json(&serde_json::from_str(&line).unwrap());
        assert_eq!(r.status, Status::Success);
        assert_eq!(r.gas_used, 42);
        assert_eq!(r.decode_return::<u64>(), 2);
        assert_eq!(r.claimed_deposit, U256::from(400u64));
        assert_eq!(r.events[0].topics, [[0xab; 32]]);
        assert_eq!(r.events[0].data, [0xde, 0xad]);
        assert_eq!(r.outbox[0].recipient, L1Address::new([0x11; 20]));
        assert_eq!(r.outbox[0].asset, AssetId::from_zts([1; 10]));
        assert_eq!(r.outbox[0].amount, U256::from(7u64));
        assert_eq!(r.state_diff[0].value, None);
        assert_eq!(r.state_diff[1].value, Some(vec![1, 2]));
    }

    #[test]
    fn assert_error_checks_code_and_fields() {
        receipt(Status::Revert, 3, &[9, 9]).assert_error(&Boom);
    }

    #[test]
    #[should_panic(expected = "expected status Revert")]
    fn assert_error_rejects_other_fields() {
        receipt(Status::Revert, 3, &[9]).assert_error(&Boom);
    }

    #[test]
    #[should_panic(expected = "expected status Revert")]
    fn assert_error_rejects_success() {
        receipt(Status::Success, 0, &[]).assert_error(&Boom);
    }

    #[test]
    #[should_panic(expected = "left 1 state writes")]
    fn assert_abort_rejects_state_writes() {
        let mut r = receipt(Status::Revert, -1, &[]);
        r.state_diff.push(StateWrite {
            path: [0; 32],
            value: None,
        });
        r.assert_abort(-1);
    }

    #[test]
    fn assert_abort_accepts_a_bare_revert() {
        receipt(Status::Revert, -1, &[]).assert_abort(-1);
    }
}
