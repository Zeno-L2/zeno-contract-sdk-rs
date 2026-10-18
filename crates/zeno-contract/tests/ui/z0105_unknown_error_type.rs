#![allow(unused)]

use zeno_contract::prelude::*;

#[contract]
mod m {
    use super::*;

    #[method]
    pub fn f(ctx: &mut Ctx) -> Result<(), u8> {
        Ok(())
    }
}

fn main() {}
