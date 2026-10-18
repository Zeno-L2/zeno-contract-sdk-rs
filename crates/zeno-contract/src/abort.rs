//! Abort codes and the abort entry points.

/// The abort codes the runtime raises.
pub mod codes {
    /// The frame, the selector or the arguments do not decode.
    pub const MALFORMED_PAYLOAD: i32 = -1;
    /// No method has the payload's selector.
    pub const UNKNOWN_SELECTOR: i32 = -2;
    /// An empty payload reached a contract with no deposit handler.
    pub const NO_DEPOSIT_HANDLER: i32 = -3;
    /// An effect broke a cap or does not fit a buffer.
    pub const CAP_EXCEEDED: i32 = -4;
    /// `claim_deposit` asked for more than the claimable amount.
    pub const CLAIM_ABOVE_DEPOSIT: i32 = -5;
    /// A stored value fails to decode.
    pub const STORED_VALUE_DECODE: i32 = -6;
    /// Reserved, never raised.
    pub const ALLOCATION_FAILURE: i32 = -7;
    /// The module panicked.
    pub const PANIC: i32 = -8;
    /// `withdraw` to an address that is not a user address.
    pub const INVALID_RECIPIENT: i32 = -9;
}

/// The most revert data an abort carries.
const MAX_REVERT_DATA: usize = 4096;

/// Aborts the call with `code` and no revert data.
pub(crate) fn abort(code: i32) -> ! {
    crate::env::abort_raw(code, &[])
}

/// Aborts the call with `code` and `data` as the revert data. Data
/// above 4096 bytes aborts with the cap code instead.
pub(crate) fn abort_with(code: i32, data: &[u8]) -> ! {
    if data.len() > MAX_REVERT_DATA {
        abort(codes::CAP_EXCEEDED);
    }
    crate::env::abort_raw(code, data)
}

/// Aborts with the panic code. It never reads the panic info, so no
/// formatting code links into the module.
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    abort(codes::PANIC)
}
