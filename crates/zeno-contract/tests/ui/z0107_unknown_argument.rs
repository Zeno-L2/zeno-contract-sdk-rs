#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[method(fast)]
    pub fn f(ctx: &mut Ctx) {}
}

fn main() {}
