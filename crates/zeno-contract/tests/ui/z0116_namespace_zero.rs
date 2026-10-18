#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[storage(0)]
    const COUNT: Item<u64> = Item::new();
}

fn main() {}
