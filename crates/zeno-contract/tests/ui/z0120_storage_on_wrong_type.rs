#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[storage(0x01)]
    const COUNT: u64 = 0;
}

fn main() {}
