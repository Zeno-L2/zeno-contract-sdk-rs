//! A static layout check over built contract modules. It reads the
//! space-separated absolute paths in `ZENO_LAYOUT_WASM` and checks that
//! each module has one memory of one initial page, a stack pointer
//! starting at 32768, and only data segments above the stack. When the
//! variable is unset or empty the test checks nothing, unless
//! `ZENO_REQUIRE_WASM_TESTS` is 1, which makes it fail.

use std::fs;

const STACK_TOP: u64 = 32768;
const PAGE: u64 = 65536;

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor { bytes, pos: 0 }
    }

    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.pos).ok_or("unexpected end of module")?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("length overflow")?;
        let s = self
            .bytes
            .get(self.pos..end)
            .ok_or("unexpected end of module")?;
        self.pos = end;
        Ok(s)
    }

    fn uleb(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.byte()?;
            if shift >= 64 {
                return Err("LEB128 value too long".into());
            }
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
        }
    }

    fn sleb(&mut self) -> Result<i64, String> {
        let mut value = 0i64;
        let mut shift = 0u32;
        loop {
            let b = self.byte()?;
            if shift < 64 {
                value |= i64::from(b & 0x7f) << shift;
            }
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && b & 0x40 != 0 {
                    value |= -1i64 << shift;
                }
                return Ok(value);
            }
            if shift > 70 {
                return Err("LEB128 value too long".into());
            }
        }
    }

    fn at_end(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

struct Layout {
    initial_pages: u64,
    stack_pointer: i64,
    segments: Vec<(u64, u64)>,
}

/// Reads an `i32.const <n>; end` init expression and returns `n`.
fn const_expr(c: &mut Cursor) -> Result<i64, String> {
    if c.byte()? != 0x41 {
        return Err("init expression is not i32.const".into());
    }
    let v = c.sleb()?;
    if c.byte()? != 0x0b {
        return Err("init expression does not end after one instruction".into());
    }
    Ok(v)
}

fn parse(bytes: &[u8]) -> Result<Layout, String> {
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return Err("not a wasm module".into());
    }
    let mut c = Cursor::new(&bytes[8..]);
    let mut memories = None;
    let mut stack_pointer = None;
    let mut segments = Vec::new();
    while !c.at_end() {
        let id = c.byte()?;
        let size = c.uleb()? as usize;
        let mut s = Cursor::new(c.take(size)?);
        match id {
            5 => {
                let count = s.uleb()?;
                let mut first = None;
                for _ in 0..count {
                    let flags = s.uleb()?;
                    let min = s.uleb()?;
                    if flags & 1 != 0 {
                        s.uleb()?;
                    }
                    first.get_or_insert(min);
                }
                memories = Some((count, first));
            }
            6 => {
                let count = s.uleb()?;
                if count == 0 {
                    return Err("global section is empty".into());
                }
                let ty = s.byte()?;
                let mutable = s.byte()?;
                if ty != 0x7f || mutable != 1 {
                    return Err("global 0 is not a mutable i32".into());
                }
                let start = s.pos;
                let v = const_expr(&mut s)?;
                if s.bytes[start..s.pos] != [0x41, 0x80, 0x80, 0x02, 0x0b] {
                    return Err("global 0 init expression is not i32.const 32768; end".into());
                }
                stack_pointer = Some(v);
            }
            11 => {
                let count = s.uleb()?;
                for i in 0..count {
                    let flag = s.uleb()?;
                    if flag != 0 {
                        return Err(format!("data segment {i} has flag {flag}, not 0"));
                    }
                    let offset =
                        const_expr(&mut s).map_err(|e| format!("data segment {i} offset: {e}"))?;
                    let len = s.uleb()?;
                    s.take(len as usize)?;
                    segments.push((offset as u64, len));
                }
            }
            _ => {}
        }
    }
    let (count, first) = memories.ok_or("no memory section")?;
    if count != 1 {
        return Err(format!("memory section declares {count} memories, not 1"));
    }
    Ok(Layout {
        initial_pages: first.unwrap_or(0),
        stack_pointer: stack_pointer.ok_or("no global section")?,
        segments,
    })
}

fn check(path: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read: {e}"))?;
    let layout = parse(&bytes)?;
    println!("{path}");
    println!("  initial pages: {}", layout.initial_pages);
    println!("  __stack_pointer: {}", layout.stack_pointer);
    for (offset, len) in &layout.segments {
        println!("  segment: offset {offset}, length {len}");
    }
    if layout.initial_pages != 1 {
        return Err(format!(
            "initial page count is {}, not 1",
            layout.initial_pages
        ));
    }
    for (offset, len) in layout.segments {
        if offset < STACK_TOP {
            return Err(format!("data segment at {offset} starts below {STACK_TOP}"));
        }
        if offset + len > PAGE {
            return Err(format!("data segment {offset}+{len} ends above {PAGE}"));
        }
    }
    Ok(())
}

#[test]
fn modules_fit_one_page() {
    let paths = match std::env::var("ZENO_LAYOUT_WASM") {
        Ok(p) if !p.trim().is_empty() => p,
        _ if std::env::var("ZENO_REQUIRE_WASM_TESTS").as_deref() == Ok("1") => panic!(
            "ZENO_LAYOUT_WASM is unset or empty and ZENO_REQUIRE_WASM_TESTS is 1\nfix: build and strip the modules, then set ZENO_LAYOUT_WASM to their absolute paths"
        ),
        _ => {
            println!("ZENO_LAYOUT_WASM unset, no module checked");
            return;
        }
    };
    let mut failures = Vec::new();
    for path in paths.split_whitespace() {
        if let Err(e) = check(path) {
            failures.push(format!("{path}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "layout check failed:\n{}",
        failures.join("\n")
    );
}
