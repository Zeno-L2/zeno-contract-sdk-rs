//! A counter: `increment()` adds 1 to a u64 and returns the new count.
//! The `v2` feature adds `decrement()`.
#![no_std]

use zeno_contract::prelude::*;

#[contract]
mod counter {
    use super::*;

    /// An absent leaf is 0.
    #[storage(0x01)]
    const COUNT: Item<u64> = Item::new();

    #[contract_error]
    pub enum CounterError {
        /// The count is already 2^64 - 1.
        Overflow = 1,
        /// The count is already 0.
        #[cfg(feature = "v2")]
        Underflow = 2,
    }

    /// Adds 1 to the count and returns it.
    #[method(returns(count))]
    pub fn increment(ctx: &mut Ctx) -> Result<u64, CounterError> {
        let next = COUNT
            .may_load(ctx)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(CounterError::Overflow)?;
        COUNT.save(ctx, &next);
        Ok(next)
    }

    /// Subtracts 1 from the count and returns it.
    #[cfg(feature = "v2")]
    #[method(returns(count))]
    pub fn decrement(ctx: &mut Ctx) -> Result<u64, CounterError> {
        let next = COUNT
            .may_load(ctx)
            .unwrap_or(0)
            .checked_sub(1)
            .ok_or(CounterError::Underflow)?;
        COUNT.save(ctx, &next);
        Ok(next)
    }
}

#[cfg(test)]
mod interface_golden {
    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[cfg(not(feature = "v2"))]
    const GOLDEN: &str = include_str!("../golden/counter.interface.json");
    #[cfg(feature = "v2")]
    const GOLDEN: &str = include_str!("../golden/counter-v2.interface.json");

    #[test]
    fn interface_text_matches_the_golden_file() {
        assert_eq!(
            super::counter::__zeno_interface().to_json(ZERO_HASH),
            GOLDEN
        );
    }
}

#[cfg(test)]
mod tests;
