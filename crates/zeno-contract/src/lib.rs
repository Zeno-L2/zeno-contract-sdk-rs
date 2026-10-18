//! The runtime a Zeno contract module links against: the frame reader,
//! the value codec, typed storage and the effects encoder.
#![no_std]
#[cfg(not(target_arch = "wasm32"))]
extern crate std;

mod abort;
mod codec;
mod ctx;
mod effects;
mod env;
mod frame;
#[cfg(not(target_arch = "wasm32"))]
mod interface;
mod storage;
mod types;

pub use abort::codes;
pub use codec::{Codec, Fixed, Key, Outputs, Reader, Writer};
pub use ctx::Ctx;
pub use storage::{Item, Map};
pub use types::{
    AssetId, Caller, CodeHash, ContractId, Deposit, Hash32, L1Address, Momentum, U256,
};

use core::cell::UnsafeCell;

/// A zero-initialised static buffer. It stays in `.bss`, so it adds
/// nothing to the module's data segments.
#[repr(transparent)]
pub(crate) struct Buf<const N: usize>(UnsafeCell<[u8; N]>);

// SAFETY: a contract module runs on one thread.
unsafe impl<const N: usize> Sync for Buf<N> {}

impl<const N: usize> Buf<N> {
    pub(crate) const fn new() -> Self {
        Buf(UnsafeCell::new([0; N]))
    }

    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.0.get().cast()
    }
}

/// An error a method returns: a code and the fields that go in the
/// revert data.
pub trait ContractError {
    fn code(&self) -> i32;
    fn encode_fields(&self, w: &mut Writer<'_>);
}

/// An event: the topic hash, up to three indexed topics and the data.
pub trait Event {
    const TOPIC: [u8; 32];
    /// Writes the indexed topics to `out` and returns how many.
    fn topics(&self, out: &mut [[u8; 32]; 3]) -> usize;
    fn encode_data(&self, w: &mut Writer<'_>);
}

/// A type that fits in an indexed event topic.
#[diagnostic::on_unimplemented(
    message = "Z0114: `{Self}` cannot be an indexed event field",
    note = "fix: index only Hash32, Caller, ContractId, CodeHash, AssetId, [u8; 32] or U256, or drop #[indexed]"
)]
pub trait Topic: codec::sealed::Sealed {
    fn to_topic(&self) -> [u8; 32];
}

macro_rules! topic_impls {
    ($($t:ty => $f:ident),*) => {$(
        impl Topic for $t {
            fn to_topic(&self) -> [u8; 32] {
                self.$f()
            }
        }
    )*};
}
topic_impls!(
    Hash32 => to_bytes, Caller => to_bytes, ContractId => to_bytes,
    CodeHash => to_bytes, AssetId => to_bytes, U256 => to_be_bytes
);

impl Topic for [u8; 32] {
    fn to_topic(&self) -> [u8; 32] {
        *self
    }
}

/// Everything a contract module imports:
/// `use zeno_contract::prelude::*;`.
pub mod prelude {
    pub use zeno_contract_macros::{contract, contract_error, event, method, on_deposit, storage};

    pub use crate::{
        AssetId, Caller, CodeHash, ContractId, Ctx, Deposit, Hash32, Item, L1Address, Map,
        Momentum, U256,
    };
}

#[doc(hidden)]
pub mod __private {
    pub use crate::Topic;
    pub use crate::codec::{Codec, Fixed, Key, Outputs, Reader};
    pub use crate::codes;
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::interface::{Field, Interface, interface_file_test};

    use crate::abort::{abort_with, codes as c};
    use crate::{ContractError, Ctx, Writer, effects};

    pub fn abort(code: i32) -> ! {
        crate::abort::abort(code)
    }

    /// Reads the input frame the runtime wrote at `ptr`, `len` bytes
    /// long.
    ///
    /// # Safety
    ///
    /// Call once per instance, from `run`, with the frame pointer and
    /// length the runtime passed. The payload slice it returns points
    /// into a static buffer that a second call overwrites.
    pub unsafe fn enter(ptr: u32, len: u32) -> (Ctx, &'static [u8]) {
        crate::frame::enter(ptr, len)
    }

    /// Encodes the call's effects with `r` as the return data and
    /// returns `(ptr << 32) | len` of the result blob.
    ///
    /// # Safety
    ///
    /// Call at most once per instance, as the last step of `run`. It
    /// writes the static output buffer.
    pub unsafe fn finish<R: Outputs>(ctx: &Ctx, r: &R) -> i64 {
        // SAFETY: `OUT` is used by one call at a time on one thread.
        let out =
            unsafe { core::slice::from_raw_parts_mut(effects::OUT.as_ptr(), effects::OUT_SIZE) };
        let ptr = out.as_ptr() as usize as u64;
        let id = ctx.contract_id().to_bytes();
        let claimed = ctx.claimed.to_be_bytes();
        let len = effects::with(|e| {
            e.encode(
                out,
                ctx.input_index(),
                &id,
                |w| Outputs::encode(r, w),
                &claimed,
            )
        });
        match len {
            Ok(len) => (ptr << 32 | len as u64) as i64,
            Err(_) => abort(c::CAP_EXCEEDED),
        }
    }

    /// Aborts with the error's code and its fields as revert data.
    ///
    /// # Safety
    ///
    /// Call at most once per instance, as the last step of `run`. It
    /// writes the static output buffer.
    pub unsafe fn revert<E: ContractError>(e: &E) -> ! {
        // SAFETY: `OUT` is used by one call at a time on one thread.
        let out =
            unsafe { core::slice::from_raw_parts_mut(effects::OUT.as_ptr(), effects::OUT_SIZE) };
        let mut w = Writer::new(out);
        e.encode_fields(&mut w);
        if w.overflowed() {
            abort(c::CAP_EXCEEDED);
        }
        let n = w.len();
        abort_with(e.code(), &out[..n])
    }

    /// Makes the deposit claimable (`on`) or not.
    pub fn set_claimable(ctx: &mut Ctx, on: bool) {
        ctx.claimable = if on {
            ctx.deposit().map_or(crate::U256::ZERO, |d| d.amount)
        } else {
            crate::U256::ZERO
        };
    }
}
