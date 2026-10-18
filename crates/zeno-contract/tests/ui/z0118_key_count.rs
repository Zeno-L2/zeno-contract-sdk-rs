#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[storage(0x01, key(account))]
    const BALANCES: Map<(Caller, AssetId), U256> = Map::new();
}

fn main() {}
