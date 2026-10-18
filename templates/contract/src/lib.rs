//! A Zeno contract that stores the highest value raised so far.
#![no_std]

use zeno_contract::prelude::*;

#[contract]
mod {{name}} {
    use super::*;

    /// The highest value raised so far. An absent leaf is 0.
    #[storage(0x01)]
    const HIGHEST: Item<u64> = Item::new();

    #[contract_error]
    pub enum Error {
        /// The value is not above the stored one.
        NotHigher = 1,
    }

    /// The stored value rose from `from` to `to`.
    #[event]
    pub struct Raised {
        pub from: u64,
        pub to: u64,
    }

    /// Stores `value` when it is above the stored one and returns the
    /// previous value.
    #[method(returns(previous))]
    pub fn raise(ctx: &mut Ctx, value: u64) -> Result<u64, Error> {
        let previous = HIGHEST.may_load(ctx).unwrap_or(0);
        if value <= previous {
            return Err(Error::NotHigher);
        }
        HIGHEST.save(ctx, &value);
        ctx.emit(Raised {
            from: previous,
            to: value,
        });
        Ok(previous)
    }
}

#[cfg(test)]
mod tests;
