extern crate std;

use zeno_testing::*;

use crate::{{name}}::{Error, Raised};

#[test]
fn raise_stores_and_emits() {
    let mut d = TestDomain::new();
    let a = d.account(1);
    let c = d.deploy("{{name}}", &a);
    d.buy_storage(&c, &a, 4096).assert_success();

    let r = d.call(&c, &a, "raise", (5u64,));
    r.assert_success();
    assert_eq!(r.decode_return::<u64>(), 0);
    assert_eq!(d.storage(&c, &[0x01]), Some(5u64.to_be_bytes().to_vec()));
    assert_eq!(r.events.len(), 1);
    assert_eq!(
        r.events[0].topics[0],
        <Raised as zeno_contract::Event>::TOPIC
    );
    assert_eq!(
        r.events[0].data,
        [0u64.to_be_bytes(), 5u64.to_be_bytes()].concat()
    );
}

#[test]
fn raise_rejects_a_lower_value() {
    let mut d = TestDomain::new();
    let a = d.account(1);
    let c = d.deploy("{{name}}", &a);
    d.buy_storage(&c, &a, 4096).assert_success();

    d.call(&c, &a, "raise", (5u64,)).assert_success();
    d.call(&c, &a, "raise", (3u64,))
        .assert_error(&Error::NotHigher);
}
