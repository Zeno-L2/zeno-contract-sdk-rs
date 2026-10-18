#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[method(returns(a, b))]
    pub fn f(ctx: &mut Ctx) -> u64 {
        0
    }
}

fn main() {}
