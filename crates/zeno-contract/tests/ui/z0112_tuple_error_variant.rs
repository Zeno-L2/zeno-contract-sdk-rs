#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[contract_error]
    pub enum E {
        /// A tuple variant.
        A(u8) = 1,
    }
}

fn main() {}
