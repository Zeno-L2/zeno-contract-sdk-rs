//! A method taking `&Ctx` is listed as readonly in the interface, and
//! a method taking `&mut Ctx` is not.

use zeno_contract::prelude::*;

#[contract]
mod probe {
    #![allow(non_upper_case_globals, unused_macros)]
    use super::*;

    // Module items named like the generated locals must not change the
    // generated code.
    const v: u8 = 1;
    const len: u32 = 1;
    const ptr: u32 = 1;
    const ctx: u8 = 1;
    const out: u8 = 1;
    const e: u8 = 1;
    const payload: u8 = 1;
    const r: u8 = 1;
    const w: u8 = 1;
    const iface: u8 = 1;
    const arg0: u8 = 1;

    #[contract_error]
    pub enum Error {
        /// Always returned by `fail`.
        Failed = 1,
    }

    #[event]
    pub struct Touched {
        pub x: u8,
    }

    // A module macro named like a std macro must not capture the
    // generated assertions.
    macro_rules! assert {
        ($($t:tt)*) => {
            compile_error!("the module macro captured a generated assert")
        };
    }

    #[method]
    pub fn take(_ctx: &mut Ctx, _x: u8) {}

    #[method]
    pub fn peek(_ctx: &Ctx) {}

    #[method]
    pub fn poke(_ctx: &mut Ctx) {}

    #[method]
    pub fn fail(_ctx: &mut Ctx) -> Result<(), Error> {
        Err(Error::Failed)
    }
}

/// The text of the method object named `name`, up to its `outputs`.
fn method_text(json: &str, name: &str) -> String {
    let at = json
        .find(&format!("\"name\": \"{name}\""))
        .unwrap_or_else(|| panic!("no method {name}"));
    let rest = &json[at..];
    rest[..rest.find("\"outputs\"").unwrap()].to_string()
}

#[test]
fn shared_ctx_method_is_readonly_in_the_interface() {
    let json = probe::__zeno_interface().to_json(&"0".repeat(64));
    assert!(
        method_text(&json, "peek").contains("\"readonly\": true"),
        "peek takes &Ctx:\n{json}"
    );
    assert!(
        method_text(&json, "poke").contains("\"readonly\": false"),
        "poke takes &mut Ctx:\n{json}"
    );
}
