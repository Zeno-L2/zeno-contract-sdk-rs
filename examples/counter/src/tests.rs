//! Counter tests over `zeno-exec`, run by `cargo zeno test`.
extern crate std;

use std::string::String;
use std::vec::Vec;

use zeno_testing::*;

use crate::counter::CounterError;

const COUNT_KEY: [u8; 1] = [0x01];
const QUOTA_KEY: [u8; 2] = [0x00, 0x05];

fn asset() -> AssetId {
    AssetId::from_zts([1; 10])
}

/// A domain with `counter` deployed by `a` and storage bought for it.
fn setup() -> (TestDomain, Account, Contract) {
    let mut d = TestDomain::new();
    let a = d.account(0xA1);
    let c = d.deploy("counter", &a);
    d.buy_storage(&c, &a, 4096).assert_success();
    (d, a, c)
}

/// The `instrumented_code_hash` bytes of
/// `artifacts/<name>.hashes.json`.
fn instrumented_hash(name: &str) -> Vec<u8> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = std::format!("{dir}/artifacts/{name}.hashes.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let key = "\"instrumented_code_hash\":\"";
    let at = text.find(key).unwrap() + key.len();
    let digits: String = text[at..at + 64].into();
    (0..32)
        .map(|i| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn increment_counts_and_stores() {
    let (mut d, a, c) = setup();
    for want in 1..=2u64 {
        let r = d.call(&c, &a, "increment", ());
        r.assert_success();
        assert_eq!(r.decode_return::<u64>(), want);
        assert_eq!(r.claimed_deposit, U256::ZERO);
        let stored = want.to_be_bytes().to_vec();
        assert_eq!(d.storage(&c, &COUNT_KEY), Some(stored.clone()));
        let leaf = c.leaf_path(&COUNT_KEY);
        let quota = c.leaf_path(&QUOTA_KEY);
        assert!(
            r.state_diff
                .iter()
                .any(|w| w.path == leaf && w.value.as_deref() == Some(&stored[..]))
        );
        assert!(
            r.state_diff
                .iter()
                .all(|w| w.path == leaf || w.path == quota)
        );
    }
}

#[test]
fn deposit_without_handler_is_refunded() {
    let (mut d, a, c) = setup();
    let r = d.deposit(&c, &a, asset(), U256::from(50u64));
    r.assert_abort(codes::NO_DEPOSIT_HANDLER);
    assert_eq!(
        r.outbox,
        [Outbox {
            source_contract: [0; 32],
            recipient: a.address(),
            asset: asset(),
            amount: U256::from(50u64),
        }]
    );
}

#[test]
fn argument_byte_is_malformed() {
    let (mut d, a, c) = setup();
    let mut payload = selector("increment").to_vec();
    payload.push(0);
    d.call_raw(&c, &a, &payload, None)
        .assert_abort(codes::MALFORMED_PAYLOAD);
}

#[test]
fn short_payload_is_malformed() {
    let (mut d, a, c) = setup();
    d.call_raw(&c, &a, &[1, 2], None)
        .assert_abort(codes::MALFORMED_PAYLOAD);
}

#[test]
fn decrement_is_unknown_on_v1() {
    let (mut d, a, c) = setup();
    d.call_raw(&c, &a, &selector("decrement"), None)
        .assert_abort(codes::UNKNOWN_SELECTOR);
}

#[test]
fn attached_deposit_is_refunded() {
    let (mut d, a, c) = setup();
    let r = d.call_with_deposit(&c, &a, "increment", (), asset(), U256::from(7u64));
    r.assert_success();
    assert_eq!(r.claimed_deposit, U256::ZERO);
    assert_eq!(
        r.outbox,
        [Outbox {
            source_contract: [0; 32],
            recipient: a.address(),
            asset: asset(),
            amount: U256::from(7u64),
        }]
    );
}

#[test]
fn overflow_at_max() {
    let (mut d, a, c) = setup();
    d.set_storage(&c, &COUNT_KEY, Some(&u64::MAX.to_be_bytes()));
    d.call(&c, &a, "increment", ())
        .assert_error(&CounterError::Overflow);
}

#[test]
fn short_stored_leaf_aborts() {
    let (mut d, a, c) = setup();
    d.set_storage(&c, &COUNT_KEY, Some(&[1, 2, 3, 4, 5, 6, 7]));
    d.call(&c, &a, "increment", ())
        .assert_abort(codes::STORED_VALUE_DECODE);
}

#[test]
fn upgrade_and_transfer() {
    let (mut d, a, c) = setup();
    let b = d.account(0xB2);
    d.call(&c, &a, "increment", ()).assert_success();
    let r = d.call(&c, &a, "increment", ());
    assert_eq!(r.decode_return::<u64>(), 2);

    d.transfer_ownership(&c, &a, &b).assert_success();
    assert_eq!(d.upgrade(&c, &a, "counter-v2").status, Status::Unauthorized);
    let r = d.upgrade(&c, &b, "counter-v2");
    r.assert_success();
    let mut want = instrumented_hash("counter");
    want.extend(instrumented_hash("counter-v2"));
    assert_eq!(r.return_data, want);

    for (method, count) in [
        ("decrement", 1u64),
        ("increment", 2),
        ("decrement", 1),
        ("decrement", 0),
    ] {
        let r = d.call(&c, &a, method, ());
        r.assert_success();
        assert_eq!(r.decode_return::<u64>(), count);
    }
    d.call(&c, &a, "decrement", ()).assert_abort(2);
    let mut payload = selector("decrement").to_vec();
    payload.push(0);
    d.call_raw(&c, &a, &payload, None)
        .assert_abort(codes::MALFORMED_PAYLOAD);
}
