//! Vault tests over `zeno-exec`, run by `cargo zeno test`.
extern crate std;

use std::vec::Vec;

use zeno_testing::*;

use crate::vault::VaultError;

const QUOTA_KEY: [u8; 2] = [0x00, 0x05];

/// A domain with `artifact` deployed immutable by `account(0xA1)` and
/// storage bought for it.
fn setup(artifact: &str) -> (TestDomain, Account, Contract) {
    let mut d = TestDomain::new();
    let a = d.account(0xA1);
    let c = d.deploy_immutable(artifact, &a);
    d.buy_storage(&c, &a, 16384).assert_success();
    (d, a, c)
}

fn asset(n: u8) -> AssetId {
    AssetId::from_zts([n; 10])
}

/// The user address `0x00` followed by 19 bytes of `0xC3`.
fn recipient() -> L1Address {
    let mut b = [0xC3; 20];
    b[0] = 0;
    L1Address::new(b)
}

fn word(n: u64) -> [u8; 32] {
    U256::from(n).to_be_bytes()
}

fn topic(name: &str) -> [u8; 32] {
    sha3_256(&[name.as_bytes()])
}

/// The local key of `who`'s balance of `asset`.
fn balance_key(who: &Account, asset: AssetId) -> [u8; 32] {
    sha3_256(&[&[0x01], &who.caller().to_bytes(), &asset.to_bytes()])
}

/// The refund of an unclaimed deposit to `to`.
fn refund(to: &Account, asset: AssetId, n: u64) -> Outbox {
    Outbox {
        source_contract: [0; 32],
        recipient: to.address(),
        asset,
        amount: U256::from(n),
    }
}

/// The payload of a `withdraw` call with the raw argument bytes.
fn withdraw_payload(asset: &[u8], amount: u64, recipient: &[u8]) -> Vec<u8> {
    let mut p = selector("withdraw").to_vec();
    p.extend_from_slice(asset);
    p.extend_from_slice(&word(amount));
    p.extend_from_slice(recipient);
    p
}

/// Requires one state write at the leaf of `key` with `value`, and no
/// write other than the storage quota leaf.
fn check_leaf(r: &Receipt, c: &Contract, key: &[u8], value: Option<&[u8]>) {
    let leaf = c.leaf_path(key);
    let quota = c.leaf_path(&QUOTA_KEY);
    let at_leaf: Vec<_> = r.state_diff.iter().filter(|w| w.path == leaf).collect();
    assert_eq!(at_leaf.len(), 1, "state diff: {:?}", r.state_diff);
    assert_eq!(at_leaf[0].value.as_deref(), value);
    assert!(
        r.state_diff
            .iter()
            .all(|w| w.path == leaf || w.path == quota),
        "state diff: {:?}",
        r.state_diff
    );
}

fn credits_in_full(payable_call: bool) {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    let key = balance_key(&a, asset);
    let send = |d: &mut TestDomain| {
        if payable_call {
            d.call_raw(&c, &a, &[], Some((asset, U256::from(1000u64))))
        } else {
            d.deposit(&c, &a, asset, U256::from(1000u64))
        }
    };

    let r = send(&mut d);
    r.assert_success();
    assert_eq!(r.claimed_deposit, U256::from(1000u64));
    assert!(r.outbox.is_empty());
    check_leaf(&r, &c, &key, Some(&word(1000)));
    assert_eq!(
        r.events,
        [Event {
            topics: std::vec![topic("Deposited"), a.caller().to_bytes(), asset.to_bytes()],
            data: word(1000).to_vec(),
        }]
    );

    let r = send(&mut d);
    check_leaf(&r, &c, &key, Some(&word(2000)));
}

#[test]
fn deposit_credits_in_full() {
    credits_in_full(false);
}

#[test]
fn payable_call_with_empty_payload_credits_in_full() {
    credits_in_full(true);
}

#[test]
fn two_callers_two_assets_have_distinct_leaves() {
    let (mut d, a, c) = setup("vault");
    let b = d.account(0xB2);
    let mut seen = Vec::new();
    for who in [&a, &b] {
        for n in [1, 2] {
            d.deposit(&c, who, asset(n), U256::from(10u64))
                .assert_success();
            let key = balance_key(who, asset(n));
            let leaf = c.leaf_path(&key);
            assert!(!seen.contains(&leaf), "a leaf is shared between balances");
            seen.push(leaf);
            assert_eq!(d.storage(&c, &key), Some(word(10).to_vec()));
        }
    }
}

#[test]
fn withdraw_debits_and_registers_one_withdrawal() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    let key = balance_key(&a, asset);
    d.deposit(&c, &a, asset, U256::from(1000u64))
        .assert_success();

    let r = d.call(&c, &a, "withdraw", (asset, U256::from(400u64), recipient()));
    r.assert_success();
    check_leaf(&r, &c, &key, Some(&word(600)));
    assert_eq!(
        r.outbox,
        [Outbox {
            source_contract: c.id().to_bytes(),
            recipient: recipient(),
            asset,
            amount: U256::from(400u64),
        }]
    );
    let mut data = word(400).to_vec();
    data.extend_from_slice(&recipient().to_bytes());
    assert_eq!(
        r.events,
        [Event {
            topics: std::vec![topic("Withdrawn"), a.caller().to_bytes(), asset.to_bytes()],
            data,
        }]
    );

    let r = d.call(&c, &a, "withdraw", (asset, U256::from(600u64), recipient()));
    r.assert_success();
    check_leaf(&r, &c, &key, None);
    assert_eq!(d.storage(&c, &key), None);
}

#[test]
fn insufficient_balance_reverts_with_its_fields() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.deposit(&c, &a, asset, U256::from(1000u64))
        .assert_success();
    d.call(&c, &a, "withdraw", (asset, U256::from(400u64), recipient()))
        .assert_success();

    let r = d.call_with_deposit(
        &c,
        &a,
        "withdraw",
        (asset, U256::from(1500u64), recipient()),
        asset,
        U256::from(50u64),
    );
    r.assert_error(&VaultError::InsufficientBalance {
        asset,
        requested: U256::from(1500u64),
        available: U256::from(600u64),
    });
    let mut data = asset.to_bytes().to_vec();
    data.extend_from_slice(&word(1500));
    data.extend_from_slice(&word(600));
    assert_eq!(r.return_data.len(), 96);
    assert_eq!(r.return_data, data);
    assert_eq!(r.outbox, [refund(&a, asset, 50)]);
}

#[test]
fn attached_deposit_on_withdraw_is_refunded() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.deposit(&c, &a, asset, U256::from(1000u64))
        .assert_success();

    let r = d.call_with_deposit(
        &c,
        &a,
        "withdraw",
        (asset, U256::from(400u64), recipient()),
        asset,
        U256::from(50u64),
    );
    r.assert_success();
    assert_eq!(r.claimed_deposit, U256::ZERO);
    assert_eq!(r.outbox.len(), 2);
    assert_eq!(r.outbox[0].source_contract, c.id().to_bytes());
    assert_eq!(r.outbox[1], refund(&a, asset, 50));
}

#[test]
fn rejected_withdrawals_leave_the_balance() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    let key = balance_key(&a, asset);
    d.deposit(&c, &a, asset, U256::from(1000u64))
        .assert_success();
    let ab = asset.to_bytes();
    let call = |d: &mut TestDomain, payload: &[u8]| d.call_raw(&c, &a, payload, None);

    call(&mut d, &withdraw_payload(&ab, 0, &recipient().to_bytes()))
        .assert_error(&VaultError::ZeroAmount);

    // A payload one byte short of the recipient is malformed.
    let mut short = selector("withdraw").to_vec();
    short.extend_from_slice(&ab);
    short.extend_from_slice(&word(1));
    short.extend_from_slice(&recipient().to_bytes()[..19]);
    assert_eq!(short.len(), 87);
    call(&mut d, &short).assert_abort(codes::MALFORMED_PAYLOAD);
    call(&mut d, &[1, 2, 3]).assert_abort(codes::MALFORMED_PAYLOAD);
    call(&mut d, &selector("claim_partial")).assert_abort(codes::UNKNOWN_SELECTOR);

    // The amount check runs before the recipient check.
    let mut embedded = [0xC3; 20];
    embedded[0] = 0x01;
    call(&mut d, &withdraw_payload(&ab, 0, &embedded)).assert_error(&VaultError::ZeroAmount);

    // The recipient check runs before the balance read, so these abort
    // with the recipient code although the amount exceeds the balance.
    for (first, rest) in [(0x01, 0xC3), (0x02, 0xC3), (0x00, 0x00)] {
        let mut bad = [rest; 20];
        bad[0] = first;
        let r = call(&mut d, &withdraw_payload(&ab, 5000, &bad));
        r.assert_error(&VaultError::InvalidRecipient);
        assert!(r.outbox.is_empty());
    }
    assert_eq!(d.storage(&c, &key), Some(word(1000).to_vec()));
}

#[test]
fn withdraw_from_an_absent_leaf_reverts() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.call(&c, &a, "withdraw", (asset, U256::from(5u64), recipient()))
        .assert_error(&VaultError::InsufficientBalance {
            asset,
            requested: U256::from(5u64),
            available: U256::ZERO,
        });
}

#[test]
fn zero_value_call_changes_nothing() {
    let (mut d, a, c) = setup("vault");
    let r = d.call_raw(&c, &a, &[], None);
    r.assert_success();
    assert_eq!(r.claimed_deposit, U256::ZERO);
    assert!(r.state_diff.is_empty());
    assert!(r.events.is_empty());
    assert!(r.outbox.is_empty());
}

#[test]
fn balances_carry_across_limbs() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    let key = balance_key(&a, asset);
    d.deposit(&c, &a, asset, U256::from(u64::MAX))
        .assert_success();
    d.deposit(&c, &a, asset, U256::from(1u64)).assert_success();
    let mut two_64 = [0u8; 32];
    two_64[23] = 1;
    assert_eq!(d.storage(&c, &key), Some(two_64.to_vec()));

    d.call(&c, &a, "withdraw", (asset, U256::from(1u64), recipient()))
        .assert_success();
    assert_eq!(d.storage(&c, &key), Some(word(u64::MAX).to_vec()));
}

#[test]
fn balance_overflow_is_refunded() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.set_storage(&c, &balance_key(&a, asset), Some(&[0xFF; 32]));
    let r = d.deposit(&c, &a, asset, U256::from(1u64));
    r.assert_error(&VaultError::BalanceOverflow);
    assert_eq!(r.outbox, [refund(&a, asset, 1)]);
}

#[test]
fn short_stored_leaf_is_refunded() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.set_storage(&c, &balance_key(&a, asset), Some(&[0; 31]));
    let r = d.deposit(&c, &a, asset, U256::from(5u64));
    r.assert_abort(codes::STORED_VALUE_DECODE);
    assert_eq!(r.outbox, [refund(&a, asset, 5)]);
}

#[test]
fn deposit_before_deploy_is_refunded() {
    let mut d = TestDomain::new();
    let a = d.account(0xA1);
    let mut id = [0u8; 32];
    id[0] = 0xEE;
    let c = d.contract_at(ContractId::new(id));
    let r = d.deposit(&c, &a, asset(1), U256::from(5u64));
    assert_eq!(r.status, Status::ValidationFailed);
    assert_eq!(r.gas_used, 0);
    assert_eq!(r.outbox, [refund(&a, asset(1), 5)]);
}

#[test]
fn deposit_without_storage_is_refunded() {
    let mut d = TestDomain::new();
    let a = d.account(0xA1);
    let c = d.deploy_immutable("vault", &a);
    let r = d.deposit(&c, &a, asset(1), U256::from(5u64));
    assert_eq!(r.status, Status::StorageQuotaExceeded);
    assert!(r.events.is_empty());
    assert!(r.state_diff.is_empty());
    assert_eq!(r.outbox, [refund(&a, asset(1), 5)]);
}

#[test]
fn test_hook_partial_claim() {
    let (mut d, a, c) = setup("vault-testhook");
    let asset = asset(1);
    let r = d.call_with_deposit(
        &c,
        &a,
        "claim_partial",
        (U256::from(400u64),),
        asset,
        U256::from(1000u64),
    );
    r.assert_success();
    assert_eq!(r.claimed_deposit, U256::from(400u64));
    check_leaf(&r, &c, &balance_key(&a, asset), Some(&word(400)));
    assert_eq!(
        r.events,
        [Event {
            topics: std::vec![topic("Deposited"), a.caller().to_bytes(), asset.to_bytes()],
            data: word(400).to_vec(),
        }]
    );
    assert_eq!(r.outbox, [refund(&a, asset, 600)]);
}

#[test]
fn test_hook_claim_above_deposit_reverts() {
    let (mut d, a, c) = setup("vault-testhook");
    let asset = asset(1);
    let r = d.call_with_deposit(
        &c,
        &a,
        "claim_partial",
        (U256::from(1001u64),),
        asset,
        U256::from(1000u64),
    );
    r.assert_abort(codes::CLAIM_ABOVE_DEPOSIT);
    assert_eq!(r.outbox, [refund(&a, asset, 1000)]);
}

#[test]
fn vault_artifact_has_no_hook() {
    let (mut d, a, c) = setup("vault");
    let mut payload = selector("claim_partial").to_vec();
    payload.extend_from_slice(&word(400));
    d.call_raw(&c, &a, &payload, Some((asset(1), U256::from(1000u64))))
        .assert_abort(codes::UNKNOWN_SELECTOR);
}

#[test]
fn asset_id_with_high_bytes_is_malformed() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.deposit(&c, &a, asset, U256::from(1000u64))
        .assert_success();

    // An asset id with a non-zero byte in [0:22] fails argument decode,
    // so the call has no effects at all.
    let payload = withdraw_payload(&[1; 32], 5, &recipient().to_bytes());
    let r = d.call_raw(&c, &a, &payload, None);
    r.assert_abort(codes::MALFORMED_PAYLOAD);
    assert!(r.state_diff.is_empty());
    assert!(r.events.is_empty());
    assert!(r.outbox.is_empty());
    assert_eq!(r.claimed_deposit, U256::ZERO);
    assert_eq!(
        d.storage(&c, &balance_key(&a, asset)),
        Some(word(1000).to_vec())
    );
}

#[test]
fn argument_checks_run_before_the_stored_balance_is_read() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    let ab = asset.to_bytes();
    d.set_storage(&c, &balance_key(&a, asset), Some(&[0; 31]));
    let call = |d: &mut TestDomain, payload: &[u8]| d.call_raw(&c, &a, payload, None);

    // A zero amount and an invalid recipient return their own codes
    // although the stored balance would abort the read.
    let mut embedded = [0xC3; 20];
    embedded[0] = 0x01;
    call(&mut d, &withdraw_payload(&ab, 0, &recipient().to_bytes()))
        .assert_error(&VaultError::ZeroAmount);
    call(&mut d, &withdraw_payload(&ab, 5, &embedded)).assert_error(&VaultError::InvalidRecipient);
    call(&mut d, &withdraw_payload(&ab, 5, &[0; 20])).assert_error(&VaultError::InvalidRecipient);

    // With valid arguments the balance is read and its width aborts.
    call(&mut d, &withdraw_payload(&ab, 5, &recipient().to_bytes()))
        .assert_abort(codes::STORED_VALUE_DECODE);
}

#[test]
fn trailing_payload_bytes_are_malformed() {
    let (mut d, a, c) = setup("vault");
    let asset = asset(1);
    d.deposit(&c, &a, asset, U256::from(1000u64))
        .assert_success();

    let mut payload = withdraw_payload(&asset.to_bytes(), 5, &recipient().to_bytes());
    payload.push(0);
    let r = d.call_raw(&c, &a, &payload, None);
    r.assert_abort(codes::MALFORMED_PAYLOAD);
    assert!(r.outbox.is_empty());
    assert_eq!(
        d.storage(&c, &balance_key(&a, asset)),
        Some(word(1000).to_vec())
    );
}
