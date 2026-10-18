//! A map is listed as hashed in the interface exactly when its key
//! fields sum to more than 30 bytes.

use zeno_contract::prelude::*;

#[contract]
mod probe {
    use super::*;

    #[storage(0x01, key(a, b, c))]
    const KEY_29: Map<(L1Address, u64, u8), u8> = Map::new();

    #[storage(0x02, key(a, b, c))]
    const KEY_30: Map<(L1Address, u64, u16), u8> = Map::new();

    #[storage(0x03, key(a, b, c, d))]
    const KEY_31: Map<(L1Address, u64, u16, u8), u8> = Map::new();

    #[storage(0x04, key(a, b))]
    const KEY_32: Map<(u128, u128), u8> = Map::new();

    #[method]
    pub fn touch(_ctx: &mut Ctx) {}
}

/// The `hashed` value of the storage entry named `name`.
fn hashed(json: &str, name: &str) -> bool {
    let at = json
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("no storage entry {name}"));
    let rest = &json[at..];
    let at = rest.find("\"hashed\": ").unwrap() + "\"hashed\": ".len();
    rest[at..].starts_with("true")
}

#[test]
fn keys_over_30_bytes_are_listed_as_hashed() {
    let json = probe::__zeno_interface().to_json(&"0".repeat(64));
    assert!(!hashed(&json, "key_29"), "29 key bytes:\n{json}");
    assert!(!hashed(&json, "key_30"), "30 key bytes:\n{json}");
    assert!(hashed(&json, "key_31"), "31 key bytes:\n{json}");
    assert!(hashed(&json, "key_32"), "32 key bytes:\n{json}");
}
