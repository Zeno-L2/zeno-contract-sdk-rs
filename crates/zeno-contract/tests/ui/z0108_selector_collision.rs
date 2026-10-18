#![allow(unused)]

use zeno_contract::prelude::*;

// SHA3-256("m4021") and SHA3-256("m39217") both start with ccc87c12.
#[contract]
mod m {
    use super::*;

    #[method]
    pub fn m4021(ctx: &mut Ctx) {}

    #[method]
    pub fn m39217(ctx: &mut Ctx) {}
}

fn main() {}
