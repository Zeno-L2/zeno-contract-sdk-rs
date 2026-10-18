#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[event]
    pub struct Ev {
        #[indexed]
        a: Hash32,
        #[indexed]
        b: Hash32,
        #[indexed]
        c: Hash32,
        #[indexed]
        d: Hash32,
    }
}

fn main() {}
