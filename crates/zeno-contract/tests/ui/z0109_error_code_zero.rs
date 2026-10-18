#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[contract_error]
    pub enum E {
        /// Zero is not a valid code.
        Bad = 0,
    }
}

fn main() {}
