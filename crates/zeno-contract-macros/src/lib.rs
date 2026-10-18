//! The attribute macros of `zeno-contract`. `#[contract]` reads a
//! module and generates the exported `run` function, the interface
//! description and the checks around them.

mod expand;
mod parse;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::ToTokens;

/// Turns an inline module of `#[method]`, `#[on_deposit]`,
/// `#[contract_error]`, `#[event]` and `#[storage]` items into a Zeno
/// contract.
#[proc_macro_attribute]
pub fn contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = TokenStream2::from(item);
    let attr = TokenStream2::from(attr);
    let result = match syn::parse2::<syn::Item>(item.clone()) {
        Ok(syn::Item::Mod(module)) if attr.is_empty() && module.content.is_some() => {
            parse::parse(module).map(|model| expand::expand(&model))
        }
        _ => Err(parse::error(
            "Z0102",
            &item,
            "`#[contract]` applies only to an inline `mod name { ... }` and takes no arguments",
            "write `#[contract] mod name { use super::*; ... }`",
        )),
    };
    match result {
        Ok(tokens) => tokens.into(),
        Err(e) => {
            // The input item is dropped so its helper attributes do not
            // add more errors.
            e.to_compile_error().into()
        }
    }
}

/// The helper attributes only mean something inside a `#[contract]`
/// module, which removes them before they run.
fn outside(item: TokenStream) -> TokenStream {
    let item = TokenStream2::from(item);
    let error = parse::error(
        "Z0101",
        &item,
        "this attribute works only inside a `#[contract]` module",
        "move the item into the module",
    );
    let mut out = error.to_compile_error();
    item.to_tokens(&mut out);
    out.into()
}

/// Marks a free function as a dispatched method. Arguments:
/// `#[method]`, `#[method(payable)]`, `#[method(returns(a, b))]` and
/// `#[method(payable, returns(a))]`. `returns` names the outputs.
#[proc_macro_attribute]
pub fn method(_attr: TokenStream, item: TokenStream) -> TokenStream {
    outside(item)
}

/// Marks the function that handles an empty payload.
#[proc_macro_attribute]
pub fn on_deposit(_attr: TokenStream, item: TokenStream) -> TokenStream {
    outside(item)
}

/// Marks the enum of the contract's own abort codes.
#[proc_macro_attribute]
pub fn contract_error(_attr: TokenStream, item: TokenStream) -> TokenStream {
    outside(item)
}

/// Marks a struct as an event.
#[proc_macro_attribute]
pub fn event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    outside(item)
}

/// Gives an `Item` or `Map` constant its storage namespace. Arguments:
/// `#[storage(0xNN)]` for an `Item` and `#[storage(0xNN, key(a, b))]`
/// for a `Map`, with one key name per key field.
#[proc_macro_attribute]
pub fn storage(_attr: TokenStream, item: TokenStream) -> TokenStream {
    outside(item)
}
