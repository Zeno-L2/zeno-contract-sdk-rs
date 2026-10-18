//! The wire encoding of contract values.

use crate::types::{AssetId, Caller, CodeHash, ContractId, Hash32, L1Address, U256};

/// Keeps the codec traits closed: only the types this crate implements
/// them for can be contract values, so the interface file cannot name a
/// type with a different width than it encodes.
pub(crate) mod sealed {
    pub trait Sealed {}
}

/// Writes into a fixed buffer. Past the end it sets a flag and writes
/// nothing more.
pub struct Writer<'b> {
    buf: &'b mut [u8],
    pos: usize,
    overflow: bool,
}

impl<'b> Writer<'b> {
    pub fn new(buf: &'b mut [u8]) -> Self {
        Writer {
            buf,
            pos: 0,
            overflow: false,
        }
    }

    /// Appends `b`, or sets the overflow flag when it does not fit.
    pub fn raw(&mut self, b: &[u8]) {
        if self.overflow {
            return;
        }
        match self.pos.checked_add(b.len()) {
            Some(end) if end <= self.buf.len() => {
                self.buf[self.pos..end].copy_from_slice(b);
                self.pos = end;
            }
            _ => self.overflow = true,
        }
    }

    /// The number of bytes written.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.pos
    }

    pub fn overflowed(&self) -> bool {
        self.overflow
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.raw(&[v]);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.raw(&v.to_be_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.raw(&v.to_be_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.raw(&v.to_be_bytes());
    }

    /// Overwrites four already written bytes at `at` with `v`.
    pub(crate) fn patch_u32(&mut self, at: usize, v: u32) {
        if !self.overflow {
            self.buf[at..at + 4].copy_from_slice(&v.to_be_bytes());
        }
    }

    /// Overwrites two already written bytes at `at` with `v`.
    pub(crate) fn patch_u16(&mut self, at: usize, v: u16) {
        if !self.overflow {
            self.buf[at..at + 2].copy_from_slice(&v.to_be_bytes());
        }
    }
}

/// Reads from a byte slice.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    /// Returns the next `n` bytes, or `None` when fewer remain.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    /// True when every byte has been taken.
    pub fn is_empty(&self) -> bool {
        self.pos == self.buf.len()
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Some(out)
    }
}

#[diagnostic::on_unimplemented(
    message = "Z0122: `{Self}` is not a supported contract type",
    note = "fix: use u8..u128, i8..i128, bool, U256, [u8; N] with N in 1..=32, Hash32, Caller, ContractId, CodeHash, AssetId, L1Address, &[u8] or &str"
)]
/// A value that encodes to and decodes from contract bytes. Implemented
/// for the supported value types only.
pub trait Codec<'a>: sealed::Sealed + Sized {
    /// The type name in the interface file.
    const ZENO_TYPE: &'static str;
    /// Appends the value's bytes to `w`.
    fn encode(&self, w: &mut Writer<'_>);
    /// Reads a value, or `None` when the bytes are not a valid one.
    fn decode(r: &mut Reader<'a>) -> Option<Self>;
}

#[diagnostic::on_unimplemented(
    message = "Z0122: `{Self}` is not a fixed-width contract type",
    note = "fix: storage values, keys and return values use u8..u128, i8..i128, bool, U256, [u8; N], Hash32, Caller, ContractId, CodeHash, AssetId or L1Address"
)]
/// A [`Codec`] type whose encoding has one fixed width.
pub trait Fixed: for<'a> Codec<'a> {
    /// The encoded width in bytes.
    const WIDTH: usize;
}

#[diagnostic::on_unimplemented(
    message = "Z0122: `{Self}` is not a storage key",
    note = "fix: a key is one fixed-width type or a tuple of 2 to 4 of them"
)]
/// A storage map key: one fixed-width type or a tuple of 2 to 4.
pub trait Key: sealed::Sealed {
    /// The number of fields.
    const FIELDS: usize;
    /// The encoded length in bytes.
    const ENCODED_LEN: usize;
    /// The interface type name of each field.
    const TYPES: &'static [&'static str];
    /// Appends the key's bytes to `w`.
    fn encode(&self, w: &mut Writer<'_>);
}

#[diagnostic::on_unimplemented(
    message = "Z0122: `{Self}` is not a method return type",
    note = "fix: return (), one fixed-width type, or a tuple of 2 to 4 of them"
)]
/// A method's return value: `()`, one fixed-width type or a tuple of 2
/// to 4.
pub trait Outputs: sealed::Sealed {
    /// The number of outputs.
    const COUNT: usize;
    /// The interface type name of each output.
    const TYPES: &'static [&'static str];
    /// Appends the outputs' bytes to `w`.
    fn encode(&self, w: &mut Writer<'_>);
    /// Reads the outputs back from return data, or `None` when the
    /// bytes are not valid outputs.
    fn decode(r: &mut Reader<'_>) -> Option<Self>
    where
        Self: Sized;
}

macro_rules! int_codec {
    ($($t:ty => $name:literal, $w:literal;)*) => {$(
        impl sealed::Sealed for $t {}
        impl<'a> Codec<'a> for $t {
            const ZENO_TYPE: &'static str = $name;
            fn encode(&self, w: &mut Writer<'_>) {
                w.raw(&self.to_be_bytes());
            }
            fn decode(r: &mut Reader<'a>) -> Option<Self> {
                Some(<$t>::from_be_bytes(r.array::<$w>()?))
            }
        }
        fixed_impls!($t, $w);
    )*};
}

macro_rules! fixed_impls {
    ($t:ty, $w:expr) => {
        impl Fixed for $t {
            const WIDTH: usize = $w;
        }
        impl Key for $t {
            const FIELDS: usize = 1;
            const ENCODED_LEN: usize = $w;
            const TYPES: &'static [&'static str] = &[<$t as Codec<'static>>::ZENO_TYPE];
            fn encode(&self, w: &mut Writer<'_>) {
                <$t as Codec<'_>>::encode(self, w);
            }
        }
        impl Outputs for $t {
            const COUNT: usize = 1;
            const TYPES: &'static [&'static str] = &[<$t as Codec<'static>>::ZENO_TYPE];
            fn encode(&self, w: &mut Writer<'_>) {
                <$t as Codec<'_>>::encode(self, w);
            }
            fn decode(r: &mut Reader<'_>) -> Option<Self> {
                <$t as Codec<'_>>::decode(r)
            }
        }
    };
}

int_codec! {
    u8 => "u8", 1;
    u16 => "u16", 2;
    u32 => "u32", 4;
    u64 => "u64", 8;
    u128 => "u128", 16;
    i8 => "i8", 1;
    i16 => "i16", 2;
    i32 => "i32", 4;
    i64 => "i64", 8;
    i128 => "i128", 16;
}

impl sealed::Sealed for bool {}
impl<'a> Codec<'a> for bool {
    const ZENO_TYPE: &'static str = "bool";
    fn encode(&self, w: &mut Writer<'_>) {
        w.u8(u8::from(*self));
    }
    fn decode(r: &mut Reader<'a>) -> Option<Self> {
        match r.take(1)?[0] {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}
fixed_impls!(bool, 1);

impl sealed::Sealed for U256 {}
impl<'a> Codec<'a> for U256 {
    const ZENO_TYPE: &'static str = "u256";
    fn encode(&self, w: &mut Writer<'_>) {
        w.raw(&self.to_be_bytes());
    }
    fn decode(r: &mut Reader<'a>) -> Option<Self> {
        Some(U256::from_be_bytes(r.array()?))
    }
}
fixed_impls!(U256, 32);

macro_rules! hash_codec {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {}
        impl<'a> Codec<'a> for $t {
            const ZENO_TYPE: &'static str = "hash";
            fn encode(&self, w: &mut Writer<'_>) {
                w.raw(&self.to_bytes());
            }
            fn decode(r: &mut Reader<'a>) -> Option<Self> {
                Some(<$t>::new(r.array()?))
            }
        }
        fixed_impls!($t, 32);
    )*};
}
hash_codec!(Hash32, Caller, ContractId, CodeHash);

impl sealed::Sealed for AssetId {}
impl<'a> Codec<'a> for AssetId {
    const ZENO_TYPE: &'static str = "bytes32";
    fn encode(&self, w: &mut Writer<'_>) {
        w.raw(&self.to_bytes());
    }
    fn decode(r: &mut Reader<'a>) -> Option<Self> {
        AssetId::from_canonical(r.array()?)
    }
}
fixed_impls!(AssetId, 32);

impl sealed::Sealed for L1Address {}
impl<'a> Codec<'a> for L1Address {
    const ZENO_TYPE: &'static str = "l1Address";
    fn encode(&self, w: &mut Writer<'_>) {
        w.raw(&self.to_bytes());
    }
    fn decode(r: &mut Reader<'a>) -> Option<Self> {
        Some(L1Address::new(r.array()?))
    }
}
fixed_impls!(L1Address, 20);

macro_rules! array_codec {
    ($($n:literal => $name:literal),*) => {$(
        impl sealed::Sealed for [u8; $n] {}
        impl<'a> Codec<'a> for [u8; $n] {
            const ZENO_TYPE: &'static str = $name;
            fn encode(&self, w: &mut Writer<'_>) {
                w.raw(self);
            }
            fn decode(r: &mut Reader<'a>) -> Option<Self> {
                r.array()
            }
        }
        fixed_impls!([u8; $n], $n);
    )*};
}
array_codec!(
    1 => "bytes1", 2 => "bytes2", 3 => "bytes3", 4 => "bytes4",
    5 => "bytes5", 6 => "bytes6", 7 => "bytes7", 8 => "bytes8",
    9 => "bytes9", 10 => "bytes10", 11 => "bytes11", 12 => "bytes12",
    13 => "bytes13", 14 => "bytes14", 15 => "bytes15", 16 => "bytes16",
    17 => "bytes17", 18 => "bytes18", 19 => "bytes19", 20 => "bytes20",
    21 => "bytes21", 22 => "bytes22", 23 => "bytes23", 24 => "bytes24",
    25 => "bytes25", 26 => "bytes26", 27 => "bytes27", 28 => "bytes28",
    29 => "bytes29", 30 => "bytes30", 31 => "bytes31", 32 => "bytes32"
);

impl sealed::Sealed for &[u8] {}
impl<'a> Codec<'a> for &'a [u8] {
    const ZENO_TYPE: &'static str = "bytes";
    fn encode(&self, w: &mut Writer<'_>) {
        w.u32(self.len() as u32);
        w.raw(self);
    }
    fn decode(r: &mut Reader<'a>) -> Option<Self> {
        let n = u32::from_be_bytes(r.array()?) as usize;
        r.take(n)
    }
}

impl sealed::Sealed for &str {}
impl<'a> Codec<'a> for &'a str {
    const ZENO_TYPE: &'static str = "string";
    fn encode(&self, w: &mut Writer<'_>) {
        w.u32(self.len() as u32);
        w.raw(self.as_bytes());
    }
    fn decode(r: &mut Reader<'a>) -> Option<Self> {
        let n = u32::from_be_bytes(r.array()?) as usize;
        core::str::from_utf8(r.take(n)?).ok()
    }
}

impl sealed::Sealed for () {}
impl Outputs for () {
    const COUNT: usize = 0;
    const TYPES: &'static [&'static str] = &[];
    fn encode(&self, _: &mut Writer<'_>) {}
    fn decode(_: &mut Reader<'_>) -> Option<Self> {
        Some(())
    }
}

macro_rules! tuple_impls {
    ($(($($t:ident $i:tt),+) $n:literal;)*) => {$(
        impl<$($t: Fixed),+> sealed::Sealed for ($($t,)+) {}
        impl<$($t: Fixed),+> Key for ($($t,)+) {
            const FIELDS: usize = $n;
            const ENCODED_LEN: usize = 0 $(+ $t::WIDTH)+;
            const TYPES: &'static [&'static str] = &[$(<$t as Codec<'static>>::ZENO_TYPE),+];
            fn encode(&self, w: &mut Writer<'_>) {
                $(<$t as Codec<'_>>::encode(&self.$i, w);)+
            }
        }
        impl<$($t: Fixed),+> Outputs for ($($t,)+) {
            const COUNT: usize = $n;
            const TYPES: &'static [&'static str] = &[$(<$t as Codec<'static>>::ZENO_TYPE),+];
            fn encode(&self, w: &mut Writer<'_>) {
                $(<$t as Codec<'_>>::encode(&self.$i, w);)+
            }
            fn decode(r: &mut Reader<'_>) -> Option<Self> {
                Some(($(<$t as Codec<'_>>::decode(r)?,)+))
            }
        }
    )*};
}
tuple_impls! {
    (A 0, B 1) 2;
    (A 0, B 1, C 2) 3;
    (A 0, B 1, C 2, D 3) 4;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn enc_with(f: impl FnOnce(&mut Writer<'_>)) -> Vec<u8> {
        let mut buf = [0u8; 128];
        let mut w = Writer::new(&mut buf);
        f(&mut w);
        let n = w.len();
        buf[..n].to_vec()
    }

    fn enc<T: for<'a> Codec<'a>>(v: &T) -> Vec<u8> {
        enc_with(|w| v.encode(w))
    }

    fn round<T: for<'a> Codec<'a> + PartialEq + core::fmt::Debug>(v: T, bytes: &[u8]) {
        assert_eq!(enc(&v), bytes);
        let mut r = Reader::new(bytes);
        assert_eq!(T::decode(&mut r), Some(v));
        assert!(r.is_empty());
    }

    #[test]
    fn unsigned_integers_are_big_endian() {
        round(0x12u8, &[0x12]);
        round(0x1234u16, &[0x12, 0x34]);
        round(0x1234_5678u32, &[0x12, 0x34, 0x56, 0x78]);
        round(1u64, &[0, 0, 0, 0, 0, 0, 0, 1]);
        let mut b = [0u8; 16];
        b[15] = 9;
        round(9u128, &b);
    }

    #[test]
    fn signed_integers_are_twos_complement() {
        round(-1i8, &[0xff]);
        round(-2i16, &[0xff, 0xfe]);
        round(i32::MIN, &[0x80, 0, 0, 0]);
        round(-1i64, &[0xff; 8]);
        round(-1i128, &[0xff; 16]);
    }

    #[test]
    fn bool_decodes_only_zero_and_one() {
        round(true, &[1]);
        round(false, &[0]);
        assert_eq!(<bool as Codec>::decode(&mut Reader::new(&[2])), None);
    }

    #[test]
    fn fixed_types_round_trip() {
        let mut u = [0u8; 32];
        u[31] = 5;
        round(U256::from(5u64), &u);
        round(Hash32::new([3; 32]), &[3; 32]);
        round(Caller::new([4; 32]), &[4; 32]);
        round(ContractId::new([5; 32]), &[5; 32]);
        round(CodeHash::new([6; 32]), &[6; 32]);
        round(L1Address::new([7; 20]), &[7; 20]);
        round([1u8, 2, 3], &[1, 2, 3]);
        round([9u8; 32], &[9; 32]);
        let a = AssetId::from_zts([8; 10]);
        round(a, &a.to_bytes());
    }

    #[test]
    fn type_names() {
        assert_eq!(<u128 as Codec>::ZENO_TYPE, "u128");
        assert_eq!(<[u8; 20] as Codec>::ZENO_TYPE, "bytes20");
        assert_eq!(<Caller as Codec>::ZENO_TYPE, "hash");
        assert_eq!(<AssetId as Codec>::ZENO_TYPE, "bytes32");
        assert_eq!(<L1Address as Codec>::ZENO_TYPE, "l1Address");
        assert_eq!(<&str as Codec>::ZENO_TYPE, "string");
        assert_eq!(<&[u8] as Codec>::ZENO_TYPE, "bytes");
    }

    #[test]
    fn non_canonical_asset_id_fails_to_decode() {
        let mut b = [0u8; 32];
        b[21] = 1;
        assert_eq!(<AssetId as Codec>::decode(&mut Reader::new(&b)), None);
        b[21] = 0;
        assert!(<AssetId as Codec>::decode(&mut Reader::new(&b)).is_some());
    }

    #[test]
    fn bytes_and_strings_carry_a_length() {
        let v: &[u8] = &[1, 2];
        assert_eq!(enc_with(|w| v.encode(w)), [0, 0, 0, 2, 1, 2]);
        let mut r = Reader::new(&[0, 0, 0, 2, 1, 2]);
        assert_eq!(<&[u8]>::decode(&mut r), Some(&[1u8, 2][..]));
        assert_eq!(enc_with(|w| "hi".encode(w)), [0, 0, 0, 2, b'h', b'i']);
        assert_eq!(
            <&str>::decode(&mut Reader::new(&[0, 0, 0, 2, b'h', b'i'])),
            Some("hi")
        );
        assert_eq!(<&[u8]>::decode(&mut Reader::new(&[0, 0, 0, 3, 1, 2])), None);
    }

    #[test]
    fn invalid_utf8_fails_to_decode() {
        assert_eq!(<&str>::decode(&mut Reader::new(&[0, 0, 0, 1, 0xff])), None);
    }

    #[test]
    fn short_input_fails_to_decode() {
        assert_eq!(<u32 as Codec>::decode(&mut Reader::new(&[1, 2, 3])), None);
    }

    #[test]
    fn writer_overflow_sets_the_flag_and_stops_writing() {
        let mut buf = [0u8; 3];
        let mut w = Writer::new(&mut buf);
        w.raw(&[1, 2]);
        assert!(!w.overflowed());
        w.raw(&[3, 4]);
        assert!(w.overflowed());
        w.raw(&[9]);
        assert_eq!(w.len(), 2);
        assert_eq!(buf, [1, 2, 0]);
    }

    #[test]
    fn key_and_outputs_widths() {
        assert_eq!(<(Caller, AssetId) as Key>::ENCODED_LEN, 64);
        assert_eq!(<(Caller, AssetId) as Key>::FIELDS, 2);
        assert_eq!(<(Caller, AssetId) as Key>::TYPES, ["hash", "bytes32"]);
        assert_eq!(<(u64, bool) as Outputs>::COUNT, 2);
        assert_eq!(<() as Outputs>::COUNT, 0);
        assert_eq!(<u64 as Outputs>::TYPES, ["u64"]);
    }

    #[test]
    fn outputs_round_trip_through_decode() {
        fn out<T: Outputs + PartialEq + core::fmt::Debug>(v: T) {
            let bytes = enc_with(|w| Outputs::encode(&v, w));
            let mut r = Reader::new(&bytes);
            assert_eq!(T::decode(&mut r), Some(v));
            assert!(r.is_empty());
        }
        out(());
        out(7u64);
        out((U256::from(9u64), true));
        assert_eq!(<u64 as Outputs>::decode(&mut Reader::new(&[1, 2, 3])), None);
        assert_eq!(
            <(U256, bool) as Outputs>::decode(&mut Reader::new(&[0; 32])),
            None
        );
    }
}
