//! Typed storage entries over the call's effects and the host store.

use crate::abort::{abort, codes};
use crate::codec::{Codec, Fixed, Key, Reader, Writer};
use crate::ctx::Ctx;
use crate::{effects, env};
use core::marker::PhantomData;
use sha3::{Digest, Sha3_256};

/// The longest encoded key: four 32-byte fields.
const MAX_KEY_BYTES: usize = 128;
/// The largest encoded key length that stays unhashed. An unhashed
/// local key is then at most 31 bytes, shorter than any hashed key (32
/// bytes), so no two entries share a local key.
const MAX_UNHASHED: usize = 30;

/// One value at the namespace byte `ns`.
pub struct Item<V> {
    ns: u8,
    _value: PhantomData<fn() -> V>,
}

/// Values at the namespace byte `ns`, one per key.
pub struct Map<K, V> {
    ns: u8,
    _entry: PhantomData<fn() -> (K, V)>,
}

impl<V: Fixed> Item<V> {
    /// Declares the entry. `#[storage(0xNN)]` inside a `#[contract]`
    /// module supplies the namespace. Evaluated anywhere else it fails
    /// to compile with Z0119.
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        panic!(
            "Z0119: Item::new() outside a #[contract] module\nfix: add #[storage(0xNN)] inside a #[contract] module"
        )
    }

    #[doc(hidden)]
    pub const fn __with_namespace(ns: u8) -> Self {
        Item {
            ns,
            _value: PhantomData,
        }
    }

    /// The stored value, or `None` when absent. Aborts -6 when the
    /// stored bytes do not decode.
    pub fn may_load(&self, _ctx: &Ctx) -> Option<V> {
        load(&item_key(self.ns))
    }

    /// Stores `v`; a later save replaces it. Aborts -4 when the effects
    /// arena is full.
    pub fn save(&self, _ctx: &mut Ctx, v: &V) {
        store(&item_key(self.ns), Some(v));
    }

    /// Removes the value; the write encodes the empty sentinel.
    pub fn remove(&self, _ctx: &mut Ctx) {
        store::<V>(&item_key(self.ns), None);
    }
}

impl<K: Key, V: Fixed> Map<K, V> {
    /// Declares the entry. `#[storage(0xNN, key(a, b))]` inside a
    /// `#[contract]` module supplies the namespace and key names.
    /// Evaluated anywhere else it fails to compile with Z0119.
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        panic!(
            "Z0119: Map::new() outside a #[contract] module\nfix: add #[storage(0xNN)] inside a #[contract] module"
        )
    }

    #[doc(hidden)]
    pub const fn __with_namespace(ns: u8) -> Self {
        Map {
            ns,
            _entry: PhantomData,
        }
    }

    /// The value stored under `k`, or `None` when absent. Aborts -6
    /// when the stored bytes do not decode.
    pub fn may_load(&self, _ctx: &Ctx, k: &K) -> Option<V> {
        let (buf, n) = local_key(self.ns, k);
        load(&buf[..n])
    }

    /// Stores `v` under `k`; a later save of the same key replaces it.
    /// Aborts -4 when the effects arena is full.
    pub fn save(&self, _ctx: &mut Ctx, k: &K, v: &V) {
        let (buf, n) = local_key(self.ns, k);
        store(&buf[..n], Some(v));
    }

    /// Removes the value under `k`; the write encodes the empty
    /// sentinel.
    pub fn remove(&self, _ctx: &mut Ctx, k: &K) {
        let (buf, n) = local_key(self.ns, k);
        store::<V>(&buf[..n], None);
    }
}

/// The local key of an item: its namespace byte.
fn item_key(ns: u8) -> [u8; 1] {
    [ns]
}

/// The local key of a map entry: `ns || key` up to 30 key bytes, else
/// `Sha3_256(ns || key)`. Returns the buffer and the key length in it.
pub(crate) fn local_key<K: Key>(ns: u8, k: &K) -> ([u8; 1 + MAX_KEY_BYTES], usize) {
    let mut buf = [0u8; 1 + MAX_KEY_BYTES];
    buf[0] = ns;
    let mut w = Writer::new(&mut buf[1..]);
    Key::encode(k, &mut w);
    seal_key(buf, K::ENCODED_LEN)
}

/// Turns `ns || key` (the key being `key_len` bytes) into the local
/// key: unchanged up to 30 key bytes, else its SHA3-256 digest.
fn seal_key(mut buf: [u8; 1 + MAX_KEY_BYTES], key_len: usize) -> ([u8; 1 + MAX_KEY_BYTES], usize) {
    if key_len <= MAX_UNHASHED {
        return (buf, 1 + key_len);
    }
    let digest = Sha3_256::digest(&buf[..1 + key_len]);
    buf[..32].copy_from_slice(&digest);
    (buf, 32)
}

/// Decodes a stored value of exactly `V::WIDTH` bytes, or aborts -6.
fn decode_stored<V: Fixed>(bytes: &[u8]) -> V {
    let mut r = Reader::new(bytes);
    match <V as Codec<'_>>::decode(&mut r) {
        Some(v) if r.is_empty() && bytes.len() == V::WIDTH => v,
        _ => abort(codes::STORED_VALUE_DECODE),
    }
}

fn load<V: Fixed>(key: &[u8]) -> Option<V> {
    let overlay = effects::with(|e| match e.lookup(key) {
        Some(None) => Some(None),
        Some(Some(bytes)) => Some(Some(decode_stored::<V>(bytes))),
        None => None,
    });
    if let Some(v) = overlay {
        return v;
    }
    let mut buf = [0u8; 32];
    let out = &mut buf[..V::WIDTH];
    match env::storage_read(key, out) {
        -1 => None,
        n if n == V::WIDTH as i64 => Some(decode_stored::<V>(out)),
        _ => abort(codes::STORED_VALUE_DECODE),
    }
}

fn store<V: Fixed>(key: &[u8], v: Option<&V>) {
    let mut buf = [0u8; 32];
    let value = match v {
        Some(v) => {
            let mut w = Writer::new(&mut buf[..V::WIDTH]);
            <V as Codec<'_>>::encode(v, &mut w);
            if w.overflowed() || w.len() != V::WIDTH {
                abort(codes::CAP_EXCEEDED);
            }
            Some(&buf[..V::WIDTH])
        }
        None => None,
    };
    if effects::with(|e| e.write(key, value, V::WIDTH)).is_err() {
        abort(codes::CAP_EXCEEDED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha3::{Digest, Sha3_256};

    #[test]
    fn item_key_is_the_namespace_byte() {
        assert_eq!(item_key(0x01), [0x01]);
    }

    fn digest_of(ns: u8, key: &[u8]) -> [u8; 32] {
        let mut input = std::vec![ns];
        input.extend_from_slice(key);
        Sha3_256::digest(&input).into()
    }

    #[test]
    fn keys_up_to_30_bytes_stay_unhashed() {
        for k in [[7u8; 29].as_slice(), [7u8; 30].as_slice()] {
            let mut buf = [0u8; 1 + MAX_KEY_BYTES];
            buf[0] = 0x02;
            buf[1..=k.len()].copy_from_slice(k);
            let (out, n) = seal_key(buf, k.len());
            assert_eq!(n, 1 + k.len());
            assert_eq!(&out[..n], &buf[..n]);
        }
        let (buf, n) = local_key(0x02, &[7u8; 30]);
        assert_eq!((n, buf[0], &buf[1..n]), (31, 0x02, [7u8; 30].as_slice()));
    }

    #[test]
    fn keys_of_31_32_and_33_bytes_are_hashed() {
        let (buf, n) = local_key(0x02, &[7u8; 31]);
        assert_eq!((n, &buf[..n]), (32, digest_of(0x02, &[7u8; 31]).as_slice()));
        let (buf, n) = local_key(0x02, &[7u8; 32]);
        assert_eq!((n, &buf[..n]), (32, digest_of(0x02, &[7u8; 32]).as_slice()));
        let k = ([7u8; 32], 7u8);
        let (buf, n) = local_key(0x02, &k);
        assert_eq!((n, &buf[..n]), (32, digest_of(0x02, &[7u8; 33]).as_slice()));
    }

    /// A fixed-seed xorshift stream, so the test is reproducible.
    fn bytes(state: &mut u64, n: usize) -> std::vec::Vec<u8> {
        (0..n)
            .map(|_| {
                *state ^= *state << 13;
                *state ^= *state >> 7;
                *state ^= *state << 17;
                (*state >> 24) as u8
            })
            .collect()
    }

    fn sealed(ns: u8, key: &[u8]) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 1 + MAX_KEY_BYTES];
        buf[0] = ns;
        buf[1..=key.len()].copy_from_slice(key);
        let (out, n) = seal_key(buf, key.len());
        out[..n].to_vec()
    }

    /// For every namespace byte and every key length from 29 to 33, an
    /// attacker map with a 31-byte key tries to reproduce each local
    /// key. No unhashed local key (at most 31 bytes) may equal a hashed
    /// one (32 bytes), and an attacker key built from a victim's local
    /// key must land elsewhere.
    #[test]
    fn no_unhashed_local_key_equals_a_hashed_one() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for ns in 0..=255u8 {
            for len in 29..=33usize {
                let victim = sealed(ns, &bytes(&mut state, len));
                if len > MAX_UNHASHED {
                    assert_eq!(victim.len(), 32);
                    let attacker = sealed(victim[0], &victim[1..]);
                    assert_ne!(attacker, victim, "ns {ns:#04x}, key length {len}");
                    for attacker_len in 1..=MAX_UNHASHED {
                        let raw = sealed(victim[0], &victim[1..=attacker_len]);
                        assert_ne!(raw, victim);
                    }
                } else {
                    assert!(victim.len() <= 31);
                }
            }
        }
    }

    #[test]
    fn tuple_key_concatenates_its_fields() {
        let (buf, n) = local_key(0x01, &(1u8, 2u16));
        assert_eq!(&buf[..n], [0x01, 1, 0, 2]);
    }
}
