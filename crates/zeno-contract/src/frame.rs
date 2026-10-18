//! Reads the input frame the runtime wrote into linear memory.

use crate::Buf;
use crate::abort::{abort, codes};
use crate::ctx::Ctx;
use crate::types::{AssetId, Caller, CodeHash, ContractId, Deposit, Hash32, Momentum, U256};
use core::{ptr, slice};

/// The frame header length; the payload length prefix sits at 250 and
/// the payload at 254.
const HEADER: usize = 254;
const MAX_PAYLOAD: usize = 8192;

static PAYLOAD: Buf<MAX_PAYLOAD> = Buf::new();

/// Copies `N` bytes out of linear memory at `addr`.
fn read_at<const N: usize>(addr: usize) -> [u8; N] {
    let mut out = [0u8; N];
    // SAFETY: `enter` checked that the frame holds `addr..addr + N`.
    // The frame can start at address 0, so no reference to it is formed
    // and the bytes are copied from the integer address.
    unsafe { ptr::copy_nonoverlapping(addr as *const u8, out.as_mut_ptr(), N) };
    out
}

/// Reads the frame at `ptr`, `len` bytes long. It copies every header
/// field and the payload out of the frame, so the caller reads no frame
/// byte afterwards. Aborts -1 on a short frame, a payload length that
/// is not `len - 254` or is above 8192, or a deposit asset that is not
/// a canonical asset id.
pub(crate) fn enter(ptr: u32, len: u32) -> (Ctx, &'static [u8]) {
    let len = len as usize;
    if len < HEADER {
        abort(codes::MALFORMED_PAYLOAD);
    }
    let base = ptr as usize;
    let payload_len = u32::from_be_bytes(read_at::<4>(base + 250)) as usize;
    if payload_len != len - HEADER || payload_len > MAX_PAYLOAD {
        abort(codes::MALFORMED_PAYLOAD);
    }
    let ctx = Ctx::new(
        Hash32::new(read_at(base + 2)),
        Caller::new(read_at(base + 34)),
        ContractId::new(read_at(base + 66)),
        CodeHash::new(read_at(base + 98)),
        Momentum {
            height: u64::from_be_bytes(read_at(base + 130)),
            time: u64::from_be_bytes(read_at(base + 138)),
            hash: Hash32::new(read_at(base + 146)),
        },
        u64::from_be_bytes(read_at(base + 178)),
        Deposit {
            asset: match AssetId::from_canonical(read_at(base + 186)) {
                Some(asset) => asset,
                None => abort(codes::MALFORMED_PAYLOAD),
            },
            amount: U256::from_be_bytes(read_at(base + 218)),
        },
    );
    // SAFETY: the check above bounds `payload_len` by the buffer, and
    // the frame holds `base + 254 .. base + 254 + payload_len`.
    unsafe {
        ptr::copy_nonoverlapping((base + HEADER) as *const u8, PAYLOAD.as_ptr(), payload_len);
        (ctx, slice::from_raw_parts(PAYLOAD.as_ptr(), payload_len))
    }
}
