#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[event]
    pub struct Ev(u8);
}

fn main() {}
