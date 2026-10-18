#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[method]
    pub fn f(x: u8) {}
}

fn main() {}
