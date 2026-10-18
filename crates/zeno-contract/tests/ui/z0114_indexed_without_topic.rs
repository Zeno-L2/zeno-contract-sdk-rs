#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[event]
    pub struct Ev {
        #[indexed]
        a: u64,
    }
}

fn main() {}
