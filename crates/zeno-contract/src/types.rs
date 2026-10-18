//! The value types contracts exchange with the chain.

macro_rules! hash_newtype {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Wraps 32 bytes.
            pub const fn new(b: [u8; 32]) -> Self {
                Self(b)
            }

            /// Returns the 32 bytes.
            pub const fn to_bytes(self) -> [u8; 32] {
                self.0
            }
        }
    };
}

hash_newtype!(
    /// A 32-byte hash.
    Hash32
);
hash_newtype!(
    /// The account that sent the call.
    Caller
);
hash_newtype!(
    /// The id of a deployed contract.
    ContractId
);
hash_newtype!(
    /// The hash of a contract's code.
    CodeHash
);

/// An unsigned 256-bit integer, stored big-endian so the derived
/// ordering is numeric.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct U256([u8; 32]);

impl U256 {
    /// The value 0.
    pub const ZERO: U256 = U256([0; 32]);
    /// The value 2^256 - 1.
    pub const MAX: U256 = U256([0xff; 32]);

    /// Reads a big-endian value.
    pub const fn from_be_bytes(b: [u8; 32]) -> U256 {
        U256(b)
    }

    /// Returns the big-endian bytes.
    pub const fn to_be_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Returns `None` when the sum does not fit in 256 bits.
    pub fn checked_add(self, o: U256) -> Option<U256> {
        let mut out = [0u8; 32];
        let mut carry = 0u16;
        let mut i = 32;
        while i > 0 {
            i -= 1;
            let sum = u16::from(self.0[i]) + u16::from(o.0[i]) + carry;
            out[i] = sum as u8;
            carry = sum >> 8;
        }
        if carry != 0 { None } else { Some(U256(out)) }
    }

    /// Returns `None` when `o` is above `self`.
    pub fn checked_sub(self, o: U256) -> Option<U256> {
        let mut out = [0u8; 32];
        let mut borrow = 0i16;
        let mut i = 32;
        while i > 0 {
            i -= 1;
            let mut diff = i16::from(self.0[i]) - i16::from(o.0[i]) - borrow;
            if diff < 0 {
                diff += 256;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out[i] = diff as u8;
        }
        if borrow != 0 { None } else { Some(U256(out)) }
    }

    /// Returns `None` when the product does not fit in 256 bits.
    pub fn checked_mul(self, o: U256) -> Option<U256> {
        let mut prod = [0u8; 64];
        let mut i = 32;
        while i > 0 {
            i -= 1;
            let mut carry = 0u32;
            let mut j = 32;
            while j > 0 {
                j -= 1;
                let k = i + j + 1;
                let cur = u32::from(prod[k]) + u32::from(self.0[i]) * u32::from(o.0[j]) + carry;
                prod[k] = cur as u8;
                carry = cur >> 8;
            }
            prod[i] = carry as u8;
        }
        if prod[..32].iter().any(|&b| b != 0) {
            return None;
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&prod[32..]);
        Some(U256(out))
    }

    /// True for the value 0.
    pub fn is_zero(self) -> bool {
        self.0 == [0; 32]
    }
}

impl From<u64> for U256 {
    fn from(v: u64) -> U256 {
        let mut b = [0u8; 32];
        b[24..].copy_from_slice(&v.to_be_bytes());
        U256(b)
    }
}

/// A 32-byte asset id whose first 22 bytes are zero; the last 10 bytes
/// are the ZTS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct AssetId([u8; 32]);

impl AssetId {
    pub const fn from_zts(zts: [u8; 10]) -> Self {
        let mut b = [0u8; 32];
        let mut i = 0;
        while i < 10 {
            b[22 + i] = zts[i];
            i += 1;
        }
        AssetId(b)
    }

    pub const fn to_zts(self) -> [u8; 10] {
        let mut z = [0u8; 10];
        let mut i = 0;
        while i < 10 {
            z[i] = self.0[22 + i];
            i += 1;
        }
        z
    }

    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Wraps `b`, or returns `None` when a byte in `[0:22]` is
    /// non-zero.
    pub(crate) fn from_canonical(b: [u8; 32]) -> Option<Self> {
        if b[..22].iter().any(|&x| x != 0) {
            None
        } else {
            Some(AssetId(b))
        }
    }
}

/// A 20-byte L1 address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct L1Address([u8; 20]);

impl L1Address {
    pub const fn new(b: [u8; 20]) -> Self {
        Self(b)
    }

    pub const fn to_bytes(self) -> [u8; 20] {
        self.0
    }

    /// A user address starts with byte 0 and is not all zero.
    pub fn is_user(&self) -> bool {
        self.0[0] == 0 && self.0 != [0; 20]
    }
}

/// The asset and amount attached to a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Deposit {
    pub asset: AssetId,
    pub amount: U256,
}

/// The momentum a call executes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Momentum {
    pub height: u64,
    pub time: u64,
    pub hash: Hash32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(v: u64) -> U256 {
        U256::from(v)
    }

    #[test]
    fn add_carries_across_bytes() {
        assert_eq!(u(0xff).checked_add(u(1)), Some(u(0x100)));
        assert_eq!(u(u64::MAX).checked_add(u(1)).unwrap().to_be_bytes()[23], 1);
    }

    #[test]
    fn add_overflows_at_max() {
        assert_eq!(U256::MAX.checked_add(u(1)), None);
        assert_eq!(U256::MAX.checked_add(U256::ZERO), Some(U256::MAX));
    }

    #[test]
    fn sub_borrows_across_bytes() {
        assert_eq!(u(0x100).checked_sub(u(1)), Some(u(0xff)));
        assert_eq!(u(1).checked_sub(u(2)), None);
        assert_eq!(U256::ZERO.checked_sub(U256::ZERO), Some(U256::ZERO));
    }

    #[test]
    fn mul_carries_and_overflows() {
        assert_eq!(u(0xffff).checked_mul(u(0xffff)), Some(u(0xfffe_0001)));
        assert_eq!(
            u(u64::MAX).checked_mul(u(u64::MAX)).unwrap().to_be_bytes()[0],
            0
        );
        assert_eq!(U256::MAX.checked_mul(u(2)), None);
        assert_eq!(U256::MAX.checked_mul(u(1)), Some(U256::MAX));
        assert_eq!(U256::MAX.checked_mul(U256::ZERO), Some(U256::ZERO));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(u(0x100) > u(0xff));
        assert!(U256::MAX > u(u64::MAX));
        assert!(U256::ZERO < u(1));
    }

    #[test]
    fn user_address_rule() {
        let mut one = [0u8; 20];
        one[19] = 1;
        assert!(L1Address::new(one).is_user());
        assert!(!L1Address::new([0; 20]).is_user());
        let mut high = [0u8; 20];
        high[0] = 1;
        assert!(!L1Address::new(high).is_user());
    }

    #[test]
    fn asset_id_keeps_the_zts_in_the_last_ten_bytes() {
        let a = AssetId::from_zts([7; 10]);
        assert_eq!(a.to_zts(), [7; 10]);
        assert!(a.to_bytes()[..22].iter().all(|&b| b == 0));
        assert_eq!(AssetId::from_canonical([1; 32]), None);
    }
}
