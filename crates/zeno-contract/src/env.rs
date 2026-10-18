//! The host imports a contract module links against.

#[cfg(target_arch = "wasm32")]
mod sys {
    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        pub fn storage_read(key_ptr: i32, key_len: i32, out_ptr: i32, out_len: i32) -> i64;
        pub fn abort(code: i32, data_ptr: i32, data_len: i32) -> !;
    }
}

/// Reads the leaf at `key` into `out`. Returns -1 for an absent leaf,
/// -2 when the leaf is longer than `out`, and otherwise the leaf's
/// length.
#[cfg(target_arch = "wasm32")]
pub(crate) fn storage_read(key: &[u8], out: &mut [u8]) -> i64 {
    // SAFETY: both ranges are live slices in this module's memory.
    unsafe {
        sys::storage_read(
            key.as_ptr() as i32,
            key.len() as i32,
            out.as_mut_ptr() as i32,
            out.len() as i32,
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn storage_read(_key: &[u8], _out: &mut [u8]) -> i64 {
    unreachable!("storage_read is a wasm host import")
}

/// Aborts the call with `code` and `data` as the revert data.
#[cfg(target_arch = "wasm32")]
pub(crate) fn abort_raw(code: i32, data: &[u8]) -> ! {
    // SAFETY: `data` is a live slice in this module's memory.
    unsafe { sys::abort(code, data.as_ptr() as i32, data.len() as i32) }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn abort_raw(code: i32, _data: &[u8]) -> ! {
    unreachable!("abort {code} is a wasm host import")
}
