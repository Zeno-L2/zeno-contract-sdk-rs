#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[storage(0x01)]
    const A: Item<u64> = Item::new();

    #[storage(0x01)]
    const B: Item<u64> = Item::new();
}

fn main() {}
