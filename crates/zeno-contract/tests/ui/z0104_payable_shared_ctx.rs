#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[method(payable)]
    pub fn f(ctx: &Ctx) {}
}

fn main() {}
