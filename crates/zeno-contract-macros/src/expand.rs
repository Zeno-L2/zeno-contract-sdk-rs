//! Emits the module with the generated items around it.

use crate::parse::{ErrorEnum, Handler, Method, Model};
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use sha3::{Digest, Sha3_256};
use syn::visit_mut::{self, VisitMut};
use syn::{AttrStyle, Ident, Lifetime, Type, TypeReference};

/// Rewrites every lifetime in a type to `'static`.
struct Static;

impl VisitMut for Static {
    fn visit_type_reference_mut(&mut self, r: &mut TypeReference) {
        r.lifetime = Some(Lifetime::new("'static", Span::call_site()));
        visit_mut::visit_type_reference_mut(self, r);
    }

    fn visit_lifetime_mut(&mut self, l: &mut Lifetime) {
        *l = Lifetime::new("'static", l.span());
    }
}

fn statik(ty: &Type) -> Type {
    let mut ty = ty.clone();
    Static.visit_type_mut(&mut ty);
    ty
}

/// A local name that cannot collide with the user's names. Hygiene
/// does not cover items, so a module constant of the same name would
/// turn a binding into a constant pattern. Every name carries the
/// reserved `__zeno_` prefix, and Z0103 rejects module items that use
/// it.
fn local(name: &str) -> Ident {
    Ident::new(&format!("__zeno_{name}"), Span::mixed_site())
}

fn opt_str(s: &Option<String>) -> TokenStream {
    match s {
        Some(s) => quote!(::core::option::Option::Some(#s)),
        None => quote!(::core::option::Option::None),
    }
}

fn topic(name: &str) -> Vec<u8> {
    Sha3_256::digest(name.as_bytes()).to_vec()
}

pub fn expand(model: &Model) -> TokenStream {
    let module = &model.module;
    let ident = &module.ident;
    let vis = &module.vis;
    let (_, items) = module.content.as_ref().expect("an inline module");
    let outer = module
        .attrs
        .iter()
        .filter(|a| matches!(a.style, AttrStyle::Outer));
    let inner = module
        .attrs
        .iter()
        .filter(|a| matches!(a.style, AttrStyle::Inner(_)));

    let mut generated = TokenStream::new();
    if let Some(error) = &model.error {
        generated.extend(error_impl(error));
    }
    for event in &model.events {
        generated.extend(event_impl(event));
    }
    generated.extend(run_fn(model));
    generated.extend(interface_fn(model));
    generated.extend(assertions(model));

    quote! {
        #(#outer)*
        #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
        #vis mod #ident {
            #(#inner)*
            #(#items)*
            #generated
        }

        #[cfg(test)]
        #[test]
        fn __zeno_interface_file() {
            ::zeno_contract::__private::interface_file_test(#ident::__zeno_interface)
        }
    }
}

fn error_impl(error: &ErrorEnum) -> TokenStream {
    let ident = &error.ident;
    let w = local("w");
    let codes = error.variants.iter().map(|v| {
        let cfgs = &v.cfgs;
        let name = &v.ident;
        let code = v.code;
        if v.named {
            quote!(#(#cfgs)* Self::#name { .. } => #code,)
        } else {
            quote!(#(#cfgs)* Self::#name => #code,)
        }
    });
    let fields = error.variants.iter().map(|v| {
        let cfgs = &v.cfgs;
        let name = &v.ident;
        if v.named {
            let names = v.fields.iter().map(|(n, _)| n);
            let encodes = v
                .fields
                .iter()
                .map(|(n, _)| quote!(::zeno_contract::__private::Codec::encode(#n, #w);));
            quote!(#(#cfgs)* Self::#name { #(#names),* } => { #(#encodes)* })
        } else {
            quote!(#(#cfgs)* Self::#name => {})
        }
    });
    quote! {
        impl ::zeno_contract::ContractError for #ident {
            fn code(&self) -> i32 {
                match self { #(#codes)* }
            }
            #[allow(unused_variables)]
            fn encode_fields(&self, #w: &mut ::zeno_contract::Writer<'_>) {
                match self { #(#fields)* }
            }
        }
    }
}

fn event_impl(event: &crate::parse::Event) -> TokenStream {
    let cfgs = &event.cfgs;
    let ident = &event.ident;
    let topic = topic(&ident.to_string());
    let out = local("out");
    let w = local("w");
    let indexed: Vec<&Ident> = event.fields.iter().filter(|f| f.2).map(|f| &f.0).collect();
    let topics = indexed.iter().enumerate().map(|(i, name)| {
        quote_spanned! {name.span()=>
            #out[#i] = ::zeno_contract::__private::Topic::to_topic(&self.#name);
        }
    });
    let count = indexed.len();
    let data =
        event.fields.iter().filter(|f| !f.2).map(
            |(name, _, _)| quote!(::zeno_contract::__private::Codec::encode(&self.#name, #w);),
        );
    quote! {
        #(#cfgs)*
        impl ::zeno_contract::Event for #ident {
            const TOPIC: [u8; 32] = [#(#topic),*];
            #[allow(unused_variables)]
            fn topics(&self, #out: &mut [[u8; 32]; 3]) -> usize {
                #(#topics)*
                #count
            }
            #[allow(unused_variables)]
            fn encode_data(&self, #w: &mut ::zeno_contract::Writer<'_>) {
                #(#data)*
            }
        }
    }
}

/// Calls a method or handler and turns its result into the value `run`
/// returns.
fn call_and_finish(ident: &Ident, ctx_mut: bool, args: &[Ident], fallible: bool) -> TokenStream {
    let ctx = local("ctx");
    let out = local("out");
    let v = local("v");
    let e = local("e");
    let ctx_arg = if ctx_mut {
        quote!(&mut #ctx)
    } else {
        quote!(&#ctx)
    };
    let call = quote!(#ident(#ctx_arg #(, #args)*));
    if fallible {
        quote! {
            match #call {
                ::core::result::Result::Ok(#v) => unsafe { ::zeno_contract::__private::finish(&#ctx, &#v) },
                ::core::result::Result::Err(#e) => unsafe { ::zeno_contract::__private::revert(&#e) },
            }
        }
    } else {
        quote! {
            let #out = #call;
            unsafe { ::zeno_contract::__private::finish(&#ctx, &#out) }
        }
    }
}

fn handler_body(handler: &Handler) -> TokenStream {
    let ctx = local("ctx");
    let finish = call_and_finish(&handler.ident, handler.ctx_mut, &[], handler.fallible);
    quote! {
        ::zeno_contract::__private::set_claimable(&mut #ctx, true);
        #finish
    }
}

fn arm(method: &Method) -> TokenStream {
    let cfgs = &method.cfgs;
    let ctx = local("ctx");
    let payload = local("payload");
    let r = local("r");
    let v = local("v");
    let [b0, b1, b2, b3] = method.selector;
    let payable = method.payable;
    let names: Vec<Ident> = (0..method.args.len())
        .map(|i| local(&format!("arg{i}")))
        .collect();
    let decodes = method.args.iter().zip(&names).map(|((_, ty), name)| {
        let ty = statik(ty);
        quote! {
            let #name = match <#ty as ::zeno_contract::__private::Codec<'static>>::decode(&mut #r) {
                ::core::option::Option::Some(#v) => #v,
                ::core::option::Option::None => {
                    ::zeno_contract::__private::abort(::zeno_contract::__private::codes::MALFORMED_PAYLOAD)
                }
            };
        }
    });
    let finish = call_and_finish(&method.ident, method.ctx_mut, &names, method.fallible);
    quote! {
        #(#cfgs)*
        [#b0, #b1, #b2, #b3] => {
            let mut #r = ::zeno_contract::__private::Reader::new(&#payload[4..]);
            #(#decodes)*
            if !#r.is_empty() {
                ::zeno_contract::__private::abort(::zeno_contract::__private::codes::MALFORMED_PAYLOAD);
            }
            ::zeno_contract::__private::set_claimable(&mut #ctx, #payable);
            #finish
        }
    }
}

fn run_fn(model: &Model) -> TokenStream {
    let ptr = local("ptr");
    let len = local("len");
    let ctx = local("ctx");
    let payload = local("payload");
    let arms = model.methods.iter().map(arm);
    let empty = match &model.handler {
        Some(handler) => {
            let cfgs = &handler.cfgs;
            let body = handler_body(handler);
            quote! {
                #(#cfgs)*
                {
                    return { #body };
                }
            }
        }
        None => quote!(),
    };
    quote! {
        #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
        #[allow(unreachable_code)]
        pub extern "C" fn run(#ptr: u32, #len: u32) -> i64 {
            #[allow(unused_mut)]
            let (mut #ctx, #payload) =
                unsafe { ::zeno_contract::__private::enter(#ptr, #len) };
            if #payload.is_empty() {
                #empty
                ::zeno_contract::__private::abort(::zeno_contract::__private::codes::NO_DEPOSIT_HANDLER);
            }
            if #payload.len() < 4 {
                ::zeno_contract::__private::abort(::zeno_contract::__private::codes::MALFORMED_PAYLOAD);
            }
            match [#payload[0], #payload[1], #payload[2], #payload[3]] {
                #(#arms)*
                _ => ::zeno_contract::__private::abort(::zeno_contract::__private::codes::UNKNOWN_SELECTOR),
            }
        }
    }
}

fn field(name: &str, ty: TokenStream) -> TokenStream {
    quote!(::zeno_contract::__private::Field { name: #name, ty: #ty })
}

fn zeno_type(ty: &Type) -> TokenStream {
    let ty = statik(ty);
    quote!(<#ty as ::zeno_contract::__private::Codec<'static>>::ZENO_TYPE)
}

fn interface_fn(model: &Model) -> TokenStream {
    let iface = local("iface");
    let mut stmts = Vec::new();
    for m in &model.methods {
        let cfgs = &m.cfgs;
        let name = &m.name;
        let selector = m.selector;
        let payable = m.payable;
        let readonly = !m.ctx_mut;
        let inputs = m.args.iter().map(|(n, ty)| field(n, zeno_type(ty)));
        let names = &m.returns;
        let ok = &m.ok;
        let doc = opt_str(&m.doc);
        stmts.push(quote! {
            #(#cfgs)*
            #iface.method(
                #name,
                [#(#selector),*],
                #payable,
                #readonly,
                &[#(#inputs),*],
                &[#(#names),*],
                <#ok as ::zeno_contract::__private::Outputs>::TYPES,
                #doc,
            );
        });
    }
    if let Some(h) = &model.handler {
        let cfgs = &h.cfgs;
        let name = h.ident.to_string();
        stmts.push(quote!(#(#cfgs)* #iface.on_deposit(#name);));
    }
    for e in &model.events {
        let cfgs = &e.cfgs;
        let name = e.ident.to_string();
        let topic = topic(&name);
        let fields = e.fields.iter().map(|(n, ty, indexed)| {
            let f = field(&n.to_string(), zeno_type(ty));
            quote!((#f, #indexed))
        });
        let doc = opt_str(&e.doc);
        stmts.push(quote! {
            #(#cfgs)*
            #iface.event(#name, [#(#topic),*], &[#(#fields),*], #doc);
        });
    }
    if let Some(error) = &model.error {
        for v in &error.variants {
            let cfgs = &v.cfgs;
            let name = v.ident.to_string();
            let code = v.code;
            let fields = v
                .fields
                .iter()
                .map(|(n, ty)| field(&n.to_string(), zeno_type(ty)));
            let doc = &v.doc;
            stmts.push(quote! {
                #(#cfgs)*
                #iface.error(#code, #name, &[#(#fields),*], #doc);
            });
        }
    }
    for s in &model.storage {
        let cfgs = &s.cfgs;
        let name = &s.name;
        let ns = proc_macro2::Literal::u8_suffixed(s.namespace);
        let value = zeno_type(&s.value);
        let doc = opt_str(&s.doc);
        match &s.key {
            None => stmts.push(quote!(#(#cfgs)* #iface.item(#name, #ns, #value, #doc);)),
            Some(key) => {
                let key = statik(key);
                let fields = s.key_names.iter().enumerate().map(|(i, n)| {
                    field(
                        n,
                        quote!(<#key as ::zeno_contract::__private::Key>::TYPES[#i]),
                    )
                });
                stmts.push(quote! {
                    #(#cfgs)*
                    #iface.map(
                        #name,
                        #ns,
                        &[#(#fields),*],
                        <#key as ::zeno_contract::__private::Key>::ENCODED_LEN > 30,
                        #value,
                        #doc,
                    );
                });
            }
        }
    }
    quote! {
        #[cfg(not(target_arch = "wasm32"))]
        pub(super) fn __zeno_interface() -> ::zeno_contract::__private::Interface {
            let mut #iface = ::zeno_contract::__private::Interface::new();
            #(#stmts)*
            #iface
        }
    }
}

fn assertions(model: &Model) -> TokenStream {
    let mut out = TokenStream::new();
    for m in &model.methods {
        let cfgs = &m.cfgs;
        let ok = &m.ok;
        let n = m.returns.len();
        out.extend(quote_spanned! {m.ident.span()=>
            #(#cfgs)*
            const _: () = ::core::assert!(
                <#ok as ::zeno_contract::__private::Outputs>::COUNT == #n,
                "Z0106: `returns(...)` names a different count than the return type has outputs\nfix: name each output: #[method(returns(a, b))]"
            );
        });
    }
    for s in &model.storage {
        if let Some(key) = &s.key {
            let cfgs = &s.cfgs;
            let n = s.key_names.len();
            out.extend(quote_spanned! {s.ident.span()=>
                #(#cfgs)*
                const _: () = ::core::assert!(
                    <#key as ::zeno_contract::__private::Key>::FIELDS == #n,
                    "Z0118: `key(...)` names a different count than the key has fields\nfix: name one key field per tuple element"
                );
            });
        }
    }
    out
}
