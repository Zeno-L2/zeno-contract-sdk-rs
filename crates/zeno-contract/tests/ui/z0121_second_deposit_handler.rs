#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[on_deposit]
    pub fn first(ctx: &mut Ctx) {}

    #[on_deposit]
    pub fn second(ctx: &mut Ctx) {}
}

fn main() {}
