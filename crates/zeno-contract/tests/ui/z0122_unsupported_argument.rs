#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[method]
    pub fn f(ctx: &mut Ctx, x: f32) {}
}

fn main() {}
