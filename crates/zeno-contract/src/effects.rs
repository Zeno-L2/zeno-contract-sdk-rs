//! The effects a call accumulates and their wire encoding.

use crate::Buf;
use crate::codec::Writer;
use core::cell::UnsafeCell;

const ARENA_SIZE: usize = 8192;
/// The size of the buffer the encoded effects are written into.
pub(crate) const OUT_SIZE: usize = 12288;

const MAX_EVENTS: u32 = 64;
const MAX_TOPICS: usize = 4;
const MAX_EVENT_DATA: usize = 1024;
const MAX_OUTBOX: u32 = 32;
const MAX_RETURN_DATA: usize = 4096;

const WRITE: u8 = 1;
const EVENT: u8 = 2;
const WITHDRAWAL: u8 = 3;
/// The value length marker of a removed key.
const REMOVED: u16 = 0xFFFF;
const EMPTY_VALUE: u32 = 0xFFFF_FFFF;
const RESULT_VERSION: u16 = 1;
const KIND_L1_WITHDRAWAL: u8 = 1;

/// The buffer the encoded effects and the revert data are built in.
pub(crate) static OUT: Buf<OUT_SIZE> = Buf::new();

/// The error every add returns on a cap or an arena overflow.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Full;

/// The records of one call, in one arena. Event and withdrawal records
/// append in call order; a key's write record is replaced in place.
///
/// A write record is `1, key_len u8, key, value_len u16, value bytes`.
/// A removed key has `value_len` 0xFFFF and keeps its value bytes
/// reserved, with the reserved width in the first byte. An event record
/// is `2, topic_count u8, topics, data_len u16, data`. A withdrawal
/// record is `3, recipient 20, asset 32, amount 32`.
pub(crate) struct Effects {
    arena: [u8; ARENA_SIZE],
    len: usize,
    events: u32,
    outbox: u32,
}

impl Effects {
    pub(crate) const fn new() -> Self {
        Effects {
            arena: [0; ARENA_SIZE],
            len: 0,
            events: 0,
            outbox: 0,
        }
    }

    /// The reserved value width of the write record whose value length
    /// marker is at `at`.
    fn reserved(&self, at: usize) -> usize {
        match u16::from_be_bytes([self.arena[at], self.arena[at + 1]]) {
            REMOVED => usize::from(self.arena[at + 2]),
            n => usize::from(n),
        }
    }

    /// The end of the record that starts at `pos`.
    fn record_end(&self, pos: usize) -> usize {
        match self.arena[pos] {
            WRITE => {
                let key_end = pos + 2 + usize::from(self.arena[pos + 1]);
                key_end + 2 + self.reserved(key_end)
            }
            EVENT => {
                let topics_end = pos + 2 + 32 * usize::from(self.arena[pos + 1]);
                let data_len =
                    u16::from_be_bytes([self.arena[topics_end], self.arena[topics_end + 1]]);
                topics_end + 2 + usize::from(data_len)
            }
            _ => pos + 85,
        }
    }

    /// The start of the record for `key`, as `(record start, value
    /// length marker position)`.
    fn find(&self, key: &[u8]) -> Option<(usize, usize)> {
        let mut pos = 0;
        while pos < self.len {
            if self.arena[pos] == WRITE {
                let klen = usize::from(self.arena[pos + 1]);
                if &self.arena[pos + 2..pos + 2 + klen] == key {
                    return Some((pos, pos + 2 + klen));
                }
            }
            pos = self.record_end(pos);
        }
        None
    }

    /// Records a write of `value`, or a removal when `value` is `None`,
    /// of the key whose value is `width` bytes wide. A second write of
    /// the key replaces its record in place.
    pub(crate) fn write(
        &mut self,
        key: &[u8],
        value: Option<&[u8]>,
        width: usize,
    ) -> Result<(), Full> {
        if key.len() > usize::from(u8::MAX) || width == 0 || width > usize::from(u8::MAX) {
            return Err(Full);
        }
        if value.is_some_and(|v| v.len() != width) {
            return Err(Full);
        }
        if let Some((_, marker)) = self.find(key) {
            if self.reserved(marker) != width {
                return Err(Full);
            }
            self.set_value(marker, value, width);
            return Ok(());
        }
        let size = 1 + 1 + key.len() + 2 + width;
        if self.len + size > ARENA_SIZE {
            return Err(Full);
        }
        let start = self.len;
        let marker = start + 2 + key.len();
        self.arena[start] = WRITE;
        self.arena[start + 1] = key.len() as u8;
        self.arena[start + 2..marker].copy_from_slice(key);
        self.set_value(marker, value, width);
        self.len += size;
        Ok(())
    }

    fn set_value(&mut self, marker: usize, value: Option<&[u8]>, width: usize) {
        match value {
            Some(v) => {
                self.arena[marker..marker + 2].copy_from_slice(&(width as u16).to_be_bytes());
                self.arena[marker + 2..marker + 2 + width].copy_from_slice(v);
            }
            None => {
                self.arena[marker..marker + 2].copy_from_slice(&REMOVED.to_be_bytes());
                self.arena[marker + 2] = width as u8;
            }
        }
    }

    /// The key's record, if any: `Some(None)` for a removal.
    pub(crate) fn lookup(&self, key: &[u8]) -> Option<Option<&[u8]>> {
        let (_, marker) = self.find(key)?;
        let n = u16::from_be_bytes([self.arena[marker], self.arena[marker + 1]]);
        if n == REMOVED {
            Some(None)
        } else {
            Some(Some(&self.arena[marker + 2..marker + 2 + usize::from(n)]))
        }
    }

    /// Records an event with `topics` and the data `data` writes.
    pub(crate) fn event(
        &mut self,
        topics: &[[u8; 32]],
        data: impl FnOnce(&mut Writer<'_>),
    ) -> Result<(), Full> {
        if self.events >= MAX_EVENTS || topics.len() > MAX_TOPICS {
            return Err(Full);
        }
        let start = self.len;
        let mut w = Writer::new(&mut self.arena[start..]);
        w.u8(EVENT);
        w.u8(topics.len() as u8);
        for t in topics {
            w.raw(t);
        }
        let marker = w.len();
        w.u16(0);
        data(&mut w);
        if w.overflowed() {
            return Err(Full);
        }
        let data_len = w.len() - marker - 2;
        if data_len > MAX_EVENT_DATA {
            return Err(Full);
        }
        w.patch_u16(marker, data_len as u16);
        self.len = start + w.len();
        self.events += 1;
        Ok(())
    }

    /// Records a withdrawal of `amount` of `asset` to `to`.
    pub(crate) fn withdrawal(
        &mut self,
        to: [u8; 20],
        asset: [u8; 32],
        amount: [u8; 32],
    ) -> Result<(), Full> {
        if self.outbox >= MAX_OUTBOX || self.len + 85 > ARENA_SIZE {
            return Err(Full);
        }
        let mut w = Writer::new(&mut self.arena[self.len..]);
        w.u8(WITHDRAWAL);
        w.raw(&to);
        w.raw(&asset);
        w.raw(&amount);
        self.len += 85;
        self.outbox += 1;
        Ok(())
    }

    /// Encodes the effects as the call's result blob into `out` and
    /// returns its length.
    pub(crate) fn encode(
        &self,
        out: &mut [u8],
        input_index: u64,
        contract_id: &[u8; 32],
        return_data: impl FnOnce(&mut Writer<'_>),
        claimed: &[u8; 32],
    ) -> Result<usize, Full> {
        let mut w = Writer::new(out);
        w.u16(RESULT_VERSION);
        w.u64(input_index);

        let mut writes = 0u32;
        let mut pos = 0;
        while pos < self.len {
            if self.arena[pos] == WRITE {
                writes += 1;
            }
            pos = self.record_end(pos);
        }
        w.u32(writes);
        pos = 0;
        while pos < self.len {
            if self.arena[pos] == WRITE {
                let klen = usize::from(self.arena[pos + 1]);
                let marker = pos + 2 + klen;
                w.u32(klen as u32);
                w.raw(&self.arena[pos + 2..marker]);
                match u16::from_be_bytes([self.arena[marker], self.arena[marker + 1]]) {
                    REMOVED => w.u32(EMPTY_VALUE),
                    n => {
                        w.u32(u32::from(n));
                        w.raw(&self.arena[marker + 2..marker + 2 + usize::from(n)]);
                    }
                }
            }
            pos = self.record_end(pos);
        }

        w.u32(self.events);
        let mut index = 0u32;
        pos = 0;
        while pos < self.len {
            if self.arena[pos] == EVENT {
                let topics = usize::from(self.arena[pos + 1]);
                let topics_end = pos + 2 + 32 * topics;
                let data_len = usize::from(u16::from_be_bytes([
                    self.arena[topics_end],
                    self.arena[topics_end + 1],
                ]));
                w.raw(contract_id);
                w.u64(input_index);
                w.u32(index);
                w.u32(topics as u32);
                w.raw(&self.arena[pos + 2..topics_end]);
                w.u32(data_len as u32);
                w.raw(&self.arena[topics_end + 2..topics_end + 2 + data_len]);
                index += 1;
            }
            pos = self.record_end(pos);
        }

        w.u32(self.outbox);
        index = 0;
        pos = 0;
        while pos < self.len {
            if self.arena[pos] == WITHDRAWAL {
                w.raw(contract_id);
                w.u64(input_index);
                w.u32(index);
                w.u8(KIND_L1_WITHDRAWAL);
                w.raw(&self.arena[pos + 1..pos + 85]);
                w.u32(0);
                index += 1;
            }
            pos = self.record_end(pos);
        }

        let marker = w.len();
        w.u32(0);
        return_data(&mut w);
        let data_len = w.len().saturating_sub(marker + 4);
        if data_len > MAX_RETURN_DATA {
            return Err(Full);
        }
        w.patch_u32(marker, data_len as u32);
        w.raw(claimed);
        if w.overflowed() {
            Err(Full)
        } else {
            Ok(w.len())
        }
    }
}

/// The effects of the call in progress.
struct Shared(UnsafeCell<Effects>);

// SAFETY: a contract module runs on one thread.
unsafe impl Sync for Shared {}

static SHARED: Shared = Shared(UnsafeCell::new(Effects::new()));

/// Runs `f` on the call's effects. `f` must not call `with` again.
pub(crate) fn with<R>(f: impl FnOnce(&mut Effects) -> R) -> R {
    // SAFETY: one thread, and no caller of `with` nests another call
    // inside `f`, so this is the only live reference.
    f(unsafe { &mut *SHARED.0.get() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn encode(e: &Effects, ret: &[u8], claimed: [u8; 32]) -> Result<Vec<u8>, Full> {
        let mut out = [0u8; 4096 + 4096];
        let n = e.encode(&mut out, 7, &[0xcc; 32], |w| w.raw(ret), &claimed)?;
        Ok(out[..n].to_vec())
    }

    #[test]
    fn withdrawal_blob_bytes() {
        let mut e = Effects::new();
        e.write(&[0xaa; 32], Some(&[0x11; 32]), 32).unwrap();
        e.event(&[[1; 32], [2; 32], [3; 32]], |w| w.raw(&[9, 9]))
            .unwrap();
        e.withdrawal([5; 20], [6; 32], [7; 32]).unwrap();
        let blob = encode(&e, &[], [0; 32]).unwrap();

        let mut want: Vec<u8> = Vec::new();
        want.extend([0, 1]);
        want.extend(7u64.to_be_bytes());
        want.extend(1u32.to_be_bytes());
        want.extend(32u32.to_be_bytes());
        want.extend([0xaa; 32]);
        want.extend(32u32.to_be_bytes());
        want.extend([0x11; 32]);
        want.extend(1u32.to_be_bytes());
        want.extend([0xcc; 32]);
        want.extend(7u64.to_be_bytes());
        want.extend(0u32.to_be_bytes());
        want.extend(3u32.to_be_bytes());
        want.extend([1; 32]);
        want.extend([2; 32]);
        want.extend([3; 32]);
        want.extend(2u32.to_be_bytes());
        want.extend([9, 9]);
        want.extend(1u32.to_be_bytes());
        want.extend([0xcc; 32]);
        want.extend(7u64.to_be_bytes());
        want.extend(0u32.to_be_bytes());
        want.push(1);
        want.extend([5; 20]);
        want.extend([6; 32]);
        want.extend([7; 32]);
        want.extend(0u32.to_be_bytes());
        want.extend(0u32.to_be_bytes());
        want.extend([0; 32]);
        assert_eq!(blob, want);
    }

    #[test]
    fn second_write_replaces_the_record_without_new_space() {
        let mut e = Effects::new();
        e.write(b"k", Some(&[1, 2]), 2).unwrap();
        let len = e.len;
        e.write(b"k", Some(&[3, 4]), 2).unwrap();
        assert_eq!(e.len, len);
        assert_eq!(e.lookup(b"k"), Some(Some(&[3u8, 4][..])));
    }

    #[test]
    fn remove_encodes_one_empty_entry() {
        let mut e = Effects::new();
        e.write(b"k", Some(&[1, 2]), 2).unwrap();
        e.write(b"k", None, 2).unwrap();
        assert_eq!(e.lookup(b"k"), Some(None));
        let blob = encode(&e, &[], [0; 32]).unwrap();
        let writes = &blob[10..14];
        assert_eq!(writes, 1u32.to_be_bytes());
        assert_eq!(&blob[14..19], [0, 0, 0, 1, b'k']);
        assert_eq!(&blob[19..23], [0xff; 4]);
        e.write(b"k", Some(&[5, 6]), 2).unwrap();
        assert_eq!(e.lookup(b"k"), Some(Some(&[5u8, 6][..])));
    }

    #[test]
    fn a_removed_record_keeps_its_space_walkable() {
        let mut e = Effects::new();
        e.write(b"a", None, 8).unwrap();
        e.write(b"b", Some(&[1]), 1).unwrap();
        assert_eq!(e.lookup(b"b"), Some(Some(&[1u8][..])));
        assert_eq!(e.lookup(b"c"), None);
    }

    #[test]
    fn a_key_keeps_one_width() {
        let mut e = Effects::new();
        e.write(b"k", Some(&[1, 2]), 2).unwrap();
        assert_eq!(e.write(b"k", Some(&[1]), 1), Err(Full));
    }

    #[test]
    fn event_caps() {
        let mut e = Effects::new();
        assert_eq!(e.event(&[[0; 32]; 5], |_| {}), Err(Full));
        assert_eq!(e.event(&[], |w| w.raw(&[0; 1025])), Err(Full));
        assert!(e.event(&[[0; 32]; 4], |w| w.raw(&[0; 1024])).is_ok());
        let mut e = Effects::new();
        for _ in 0..64 {
            e.event(&[], |_| {}).unwrap();
        }
        assert_eq!(e.event(&[], |_| {}), Err(Full));
    }

    #[test]
    fn outbox_cap_is_32() {
        let mut e = Effects::new();
        for _ in 0..32 {
            e.withdrawal([1; 20], [2; 32], [3; 32]).unwrap();
        }
        assert_eq!(e.withdrawal([1; 20], [2; 32], [3; 32]), Err(Full));
    }

    #[test]
    fn arena_exhaustion_is_full() {
        let mut e = Effects::new();
        let mut n = 0u32;
        loop {
            let key = n.to_be_bytes();
            if e.write(&key, Some(&[0; 32]), 32).is_err() {
                break;
            }
            n += 1;
        }
        assert_eq!(n as usize, ARENA_SIZE / (1 + 1 + 4 + 2 + 32));
        assert_eq!(e.event(&[], |w| w.raw(&[0; 33])), Err(Full));
    }

    #[test]
    fn event_overflowing_while_writing_topics_is_full() {
        let mut e = Effects::new();
        for n in 0..203u32 {
            e.write(&n.to_be_bytes(), Some(&[0; 32]), 32).unwrap();
        }
        e.write(b"k", Some(&[0; 22]), 22).unwrap();
        assert_eq!(ARENA_SIZE - e.len, 45);
        assert_eq!(e.event(&[[0; 32]; 2], |_| {}), Err(Full));
        assert_eq!(e.event(&[[0; 32]], |_| {}), Ok(()));
    }

    #[test]
    fn return_data_and_output_caps() {
        let e = Effects::new();
        assert!(encode(&e, &[0; 4096], [0; 32]).is_ok());
        assert_eq!(encode(&e, &[0; 4097], [0; 32]), Err(Full));
        let mut small = [0u8; 10];
        assert_eq!(
            e.encode(&mut small, 0, &[0; 32], |_| {}, &[0; 32]),
            Err(Full)
        );
    }
}
