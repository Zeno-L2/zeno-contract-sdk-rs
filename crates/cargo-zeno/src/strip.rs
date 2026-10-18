//! Drops the custom sections of a module and hashes the result.

use sha3::{Digest, Sha3_256};

const HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

/// Returns `module` without its custom (id 0) sections; every other
/// section is copied unchanged.
pub fn strip(module: &[u8]) -> Result<Vec<u8>, String> {
    if module.len() < HEADER.len() || module[..HEADER.len()] != HEADER {
        return Err("bad module header".into());
    }
    let mut out = HEADER.to_vec();
    let mut pos = HEADER.len();
    while pos < module.len() {
        let start = pos;
        let id = module[pos];
        pos += 1;
        let (size, n) = leb128(&module[pos..])?;
        pos += n;
        if ((module.len() - pos) as u64) < u64::from(size) {
            return Err(format!("truncated section {id} at offset {start}"));
        }
        pos += size as usize;
        if id != 0 {
            out.extend_from_slice(&module[start..pos]);
        }
    }
    Ok(out)
}

/// Decodes an unsigned LEB128 u32 and returns it with its length.
fn leb128(b: &[u8]) -> Result<(u32, usize), String> {
    let mut v: u64 = 0;
    for i in 0..5 {
        let Some(&byte) = b.get(i) else {
            return Err("truncated section size".into());
        };
        v |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            if v > u64::from(u32::MAX) {
                return Err("section size exceeds u32".into());
            }
            return Ok((v as u32, i + 1));
        }
    }
    Err("section size exceeds 5 bytes".into())
}

/// SHA3-256 of `module` as lowercase hex.
pub fn code_hash(module: &[u8]) -> String {
    let digest = Sha3_256::digest(module);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: u8, body: &[u8]) -> Vec<u8> {
        let mut s = vec![id, body.len() as u8];
        s.extend_from_slice(body);
        s
    }

    #[test]
    fn custom_sections_are_dropped_wherever_they_sit() {
        let keep_a = section(1, &[1, 2, 3]);
        let keep_b = section(3, &[9]);
        let mut input = HEADER.to_vec();
        input.extend(section(0, b"\x04name"));
        input.extend(&keep_a);
        input.extend(section(0, &[7; 10]));
        input.extend(&keep_b);
        input.extend(section(0, &[]));
        let mut want = HEADER.to_vec();
        want.extend(&keep_a);
        want.extend(&keep_b);
        assert_eq!(strip(&input).unwrap(), want);
    }

    #[test]
    fn malformed_modules_are_refused() {
        assert!(strip(&[0, 1, 2]).is_err());
        let mut bad = HEADER.to_vec();
        bad[4] = 2;
        assert!(strip(&bad).is_err());

        let mut truncated = HEADER.to_vec();
        truncated.extend([1, 5, 1, 2]);
        assert!(strip(&truncated).is_err());

        let mut long = HEADER.to_vec();
        long.extend([1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00]);
        assert!(strip(&long).is_err());

        let mut huge = HEADER.to_vec();
        huge.extend([1, 0xff, 0xff, 0xff, 0xff, 0x1f]);
        assert!(strip(&huge).is_err());
    }

    #[test]
    fn code_hash_is_sha3_256_hex() {
        assert_eq!(
            code_hash(b""),
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
        );
    }
}
