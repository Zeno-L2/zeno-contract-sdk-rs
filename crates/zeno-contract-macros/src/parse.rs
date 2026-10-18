//! Reads a `#[contract]` module into a model and enforces every rule
//! that can be checked at expansion time.

use quote::{ToTokens, quote};
use sha3::{Digest, Sha3_256};
use syn::ext::IdentExt;
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Expr, Fields, GenericArgument, GenericParam, Ident, Item, ItemMod, Lit, Meta, Pat,
    PathArguments, ReturnType, Signature, Token, Type, parenthesized,
};

/// Builds an error `"<code>: <message>\nfix: <fix>"` spanned on
/// `tokens`.
pub fn error(code: &str, tokens: &impl ToTokens, message: &str, fix: &str) -> syn::Error {
    syn::Error::new_spanned(tokens, format!("{code}: {message}\nfix: {fix}"))
}

/// Collects errors so one run reports all of them.
#[derive(Default)]
struct Errors(Option<syn::Error>);

impl Errors {
    fn push(&mut self, e: syn::Error) {
        match &mut self.0 {
            Some(all) => all.combine(e),
            None => self.0 = Some(e),
        }
    }
}

pub struct Method {
    pub cfgs: Vec<Attribute>,
    pub ident: Ident,
    pub name: String,
    pub selector: [u8; 4],
    pub payable: bool,
    pub returns: Vec<String>,
    pub ctx_mut: bool,
    pub args: Vec<(String, Type)>,
    pub ok: Type,
    pub fallible: bool,
    pub doc: Option<String>,
}

pub struct Handler {
    pub cfgs: Vec<Attribute>,
    pub ident: Ident,
    pub ctx_mut: bool,
    pub fallible: bool,
}

pub struct Variant {
    pub cfgs: Vec<Attribute>,
    pub ident: Ident,
    pub code: i32,
    pub fields: Vec<(Ident, Type)>,
    pub named: bool,
    pub doc: String,
}

pub struct ErrorEnum {
    pub ident: Ident,
    pub variants: Vec<Variant>,
}

pub struct Event {
    pub cfgs: Vec<Attribute>,
    pub ident: Ident,
    pub fields: Vec<(Ident, Type, bool)>,
    pub doc: Option<String>,
}

pub struct Storage {
    pub cfgs: Vec<Attribute>,
    pub ident: Ident,
    pub name: String,
    pub namespace: u8,
    pub key_names: Vec<String>,
    pub key: Option<Type>,
    pub value: Type,
    pub doc: Option<String>,
}

pub struct Model {
    pub module: ItemMod,
    pub methods: Vec<Method>,
    pub handler: Option<Handler>,
    pub error: Option<ErrorEnum>,
    pub events: Vec<Event>,
    pub storage: Vec<Storage>,
}

const HELPERS: [&str; 5] = ["method", "on_deposit", "contract_error", "event", "storage"];

fn last_ident(path: &syn::Path) -> Option<&Ident> {
    path.segments.last().map(|s| &s.ident)
}

fn cfgs(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .cloned()
        .collect()
}

fn doc(attrs: &[Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(l) => match &l.lit {
                    Lit::Str(s) => Some(s.value().trim().to_string()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" "))
    }
}

fn selector(name: &str) -> [u8; 4] {
    let digest = Sha3_256::digest(name.as_bytes());
    [digest[0], digest[1], digest[2], digest[3]]
}

fn attrs_mut(item: &mut Item) -> Option<&mut Vec<Attribute>> {
    match item {
        Item::Fn(i) => Some(&mut i.attrs),
        Item::Struct(i) => Some(&mut i.attrs),
        Item::Enum(i) => Some(&mut i.attrs),
        Item::Const(i) => Some(&mut i.attrs),
        Item::Static(i) => Some(&mut i.attrs),
        Item::Type(i) => Some(&mut i.attrs),
        Item::Mod(i) => Some(&mut i.attrs),
        Item::Trait(i) => Some(&mut i.attrs),
        Item::Union(i) => Some(&mut i.attrs),
        Item::Impl(i) => Some(&mut i.attrs),
        Item::Use(i) => Some(&mut i.attrs),
        _ => None,
    }
}

fn item_ident(item: &Item) -> Option<&Ident> {
    match item {
        Item::Fn(i) => Some(&i.sig.ident),
        Item::Struct(i) => Some(&i.ident),
        Item::Enum(i) => Some(&i.ident),
        Item::Const(i) => Some(&i.ident),
        Item::Static(i) => Some(&i.ident),
        Item::Type(i) => Some(&i.ident),
        Item::Mod(i) => Some(&i.ident),
        Item::Trait(i) => Some(&i.ident),
        Item::Union(i) => Some(&i.ident),
        _ => None,
    }
}

/// Removes and returns the first helper attribute of `attrs`.
fn take_helper(attrs: &mut Vec<Attribute>) -> Option<(&'static str, Attribute)> {
    let (index, name) = attrs.iter().enumerate().find_map(|(i, a)| {
        let id = last_ident(a.path())?;
        HELPERS.iter().find(|h| id == *h).map(|h| (i, *h))
    })?;
    Some((name, attrs.remove(index)))
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(t) if t.elems.is_empty())
}

/// Splits `Item<V>` or `Map<K, V>` into its name and type arguments.
fn storage_type(ty: &Type) -> Option<(&Ident, Vec<&Type>)> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    if seg.ident != "Item" && seg.ident != "Map" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return Some((&seg.ident, Vec::new()));
    };
    let types = args
        .args
        .iter()
        .filter_map(|a| match a {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .collect();
    Some((&seg.ident, types))
}

pub fn parse(mut module: ItemMod) -> Result<Model, syn::Error> {
    let mut errs = Errors::default();
    let (_, items) = module
        .content
        .as_mut()
        .expect("the caller checked for an inline module");

    let mut model_methods: Vec<Method> = Vec::new();
    let mut handler: Option<Handler> = None;
    let mut events: Vec<Event> = Vec::new();
    let mut storage: Vec<Storage> = Vec::new();
    let mut error_enum: Option<ErrorEnum> = None;

    for item in items.iter_mut() {
        if let Some(id) = item_ident(item) {
            let name = id.unraw().to_string();
            if name == "run" || name.starts_with("__zeno") {
                errs.push(error(
                    "Z0103",
                    id,
                    &format!("`{name}` is reserved for generated code"),
                    "`run` and `__zeno*` are reserved for generated code; rename the item",
                ));
            }
        }
    }

    // The error enum comes first because method signatures name it.
    for item in items.iter_mut() {
        let Item::Enum(e) = item else { continue };
        if take_attr(&mut e.attrs, "contract_error").is_none() {
            continue;
        }
        if error_enum.is_some() {
            errs.push(error(
                "Z0112",
                &e.ident,
                "a second `#[contract_error]` enum",
                "use unit or named-field variants in one enum",
            ));
            continue;
        }
        error_enum = parse_error_enum(e, &mut errs);
    }
    let error_ident = error_enum.as_ref().map(|e| e.ident.clone());

    for item in items.iter_mut() {
        if matches!(item, Item::Enum(_)) {
            // `#[contract_error]` was handled above; any other helper
            // attribute on an enum is wrong.
            if let Item::Enum(e) = item
                && let Some((name, attr)) = take_helper(&mut e.attrs)
            {
                errs.push(misplaced(name, &attr));
            }
            continue;
        }
        let Some(attrs) = attrs_mut(item) else {
            continue;
        };
        let Some((name, attr)) = take_helper(attrs) else {
            if let Item::Const(c) = item
                && storage_type(&c.ty).is_some()
            {
                errs.push(error(
                    "Z0119",
                    &c.ident,
                    "an `Item` or `Map` constant without `#[storage]`",
                    "add `#[storage(0xNN)]` inside a `#[contract]` module",
                ));
            }
            continue;
        };
        if let Some((_, second)) = take_helper(attrs) {
            errs.push(error(
                "Z0107",
                &second,
                "a second contract attribute on one item",
                "keep one of #[method], #[on_deposit], #[contract_error], #[event] or #[storage] per item",
            ));
            continue;
        }
        match (name, item) {
            ("method", Item::Fn(f)) => {
                if let Some(m) = parse_method(f, &attr, error_ident.as_ref(), &mut errs) {
                    model_methods.push(m);
                }
            }
            ("on_deposit", Item::Fn(f)) => {
                if handler.is_some() {
                    errs.push(error(
                        "Z0121",
                        &f.sig.ident,
                        "a second `#[on_deposit]` function",
                        "keep one handler taking only `ctx`",
                    ));
                } else {
                    handler = parse_handler(f, &attr, error_ident.as_ref(), &mut errs);
                }
            }
            ("event", Item::Struct(s)) => {
                if let Some(ev) = parse_event(s, &attr, &events, &mut errs) {
                    events.push(ev);
                }
            }
            ("storage", Item::Const(c)) => {
                if let Some(s) = parse_storage(c, &attr, &mut errs) {
                    storage.push(s);
                }
            }
            (name, item) => errs.push(misplaced_on(name, item)),
        }
    }

    for (i, m) in model_methods.iter().enumerate() {
        if model_methods[..i]
            .iter()
            .any(|o| o.selector == m.selector && o.name != m.name)
        {
            errs.push(error(
                "Z0108",
                &m.ident,
                &format!(
                    "method `{}` has the same selector as an earlier method",
                    m.name
                ),
                "rename one method",
            ));
        }
    }
    if let Some(e) = &error_enum {
        for (i, v) in e.variants.iter().enumerate() {
            if e.variants[..i].iter().any(|o| o.code == v.code) {
                errs.push(error(
                    "Z0110",
                    &v.ident,
                    &format!("code {} is used by an earlier variant", v.code),
                    "give each variant its own code",
                ));
            }
        }
    }
    for (i, s) in storage.iter().enumerate() {
        if storage[..i].iter().any(|o| o.namespace == s.namespace) {
            errs.push(error(
                "Z0117",
                &s.ident,
                &format!(
                    "namespace 0x{:02x} is used by an earlier entry",
                    s.namespace
                ),
                "give each entry its own namespace",
            ));
        }
    }

    match errs.0 {
        Some(e) => Err(e),
        None => Ok(Model {
            module,
            methods: model_methods,
            handler,
            error: error_enum,
            events,
            storage,
        }),
    }
}

fn take_attr(attrs: &mut Vec<Attribute>, wanted: &str) -> Option<Attribute> {
    let index = attrs
        .iter()
        .position(|a| last_ident(a.path()).is_some_and(|i| i == wanted))?;
    Some(attrs.remove(index))
}

fn misplaced(name: &str, attr: &Attribute) -> syn::Error {
    match name {
        "method" | "on_deposit" => error(
            "Z0103",
            attr,
            "this attribute applies to a free `fn`",
            "take `ctx: &Ctx` or `ctx: &mut Ctx` first, then named arguments",
        ),
        "contract_error" => error(
            "Z0112",
            attr,
            "`#[contract_error]` applies to an enum",
            "use unit or named-field variants in one enum",
        ),
        "event" => error(
            "Z0115",
            attr,
            "`#[event]` applies to a named-field struct",
            "use a named-field struct with a unique name",
        ),
        _ => error(
            "Z0120",
            attr,
            "`#[storage]` applies to a `const`",
            "declare `const NAME: Item<V> = Item::new();`",
        ),
    }
}

fn misplaced_on(name: &str, item: &Item) -> syn::Error {
    let tokens = item_ident(item).map_or_else(|| item.to_token_stream(), |i| i.to_token_stream());
    match name {
        "method" | "on_deposit" => error(
            "Z0103",
            &tokens,
            "this attribute applies to a free `fn`",
            "take `ctx: &Ctx` or `ctx: &mut Ctx` first, then named arguments",
        ),
        "event" => error(
            "Z0115",
            &tokens,
            "`#[event]` applies to a named-field struct",
            "use a named-field struct with a unique name",
        ),
        _ => error(
            "Z0120",
            &tokens,
            "`#[storage]` applies to a `const`",
            "declare `const NAME: Item<V> = Item::new();`",
        ),
    }
}

fn parse_error_enum(e: &mut syn::ItemEnum, errs: &mut Errors) -> Option<ErrorEnum> {
    if !e.generics.params.is_empty() {
        errs.push(error(
            "Z0112",
            &e.generics,
            "the error enum has generics",
            "use unit or named-field variants in one enum",
        ));
    }
    let mut variants = Vec::new();
    for v in e.variants.iter_mut() {
        let named = match &v.fields {
            Fields::Unit => false,
            Fields::Named(_) => true,
            Fields::Unnamed(f) => {
                errs.push(error(
                    "Z0112",
                    f,
                    "a tuple variant",
                    "use unit or named-field variants in one enum",
                ));
                continue;
            }
        };
        let code = match v.discriminant.take() {
            Some((_, expr)) => match int_value(&expr) {
                Some(n) if (1..=i64::from(i32::MAX)).contains(&n) => n as i32,
                _ => {
                    errs.push(error(
                        "Z0109",
                        &expr,
                        "the code is not an integer from 1 to 2147483647",
                        "give every variant `= n` with `n >= 1`",
                    ));
                    continue;
                }
            },
            None => {
                errs.push(error(
                    "Z0109",
                    &v.ident,
                    "the variant has no `= n` code",
                    "give every variant `= n` with `n >= 1`",
                ));
                continue;
            }
        };
        let Some(description) = doc(&v.attrs) else {
            errs.push(error(
                "Z0111",
                &v.ident,
                "the variant has no `///` line",
                "add a `///` line; it becomes the interface description",
            ));
            continue;
        };
        let fields = match &v.fields {
            Fields::Named(f) => f
                .named
                .iter()
                .map(|f| (f.ident.clone().expect("named field"), f.ty.clone()))
                .collect(),
            _ => Vec::new(),
        };
        variants.push(Variant {
            cfgs: cfgs(&v.attrs),
            ident: v.ident.clone(),
            code,
            fields,
            named,
            doc: description,
        });
    }
    Some(ErrorEnum {
        ident: e.ident.clone(),
        variants,
    })
}

fn int_value(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Lit(l) => match &l.lit {
            Lit::Int(i) => i.base10_parse::<i64>().ok(),
            _ => None,
        },
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => int_value(&u.expr).map(|n| -n),
        _ => None,
    }
}

/// Checks a method or handler signature and returns whether the ctx is
/// `&mut` and the named arguments after it.
fn parse_signature(sig: &Signature, errs: &mut Errors) -> Option<(bool, Vec<(String, Type)>)> {
    let fix = "take `ctx: &Ctx` or `ctx: &mut Ctx` first, then named arguments; `run` and `__zeno*` are reserved for generated code";
    let mut ok = true;
    if sig.asyncness.is_some()
        || sig.constness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
    {
        errs.push(error(
            "Z0103",
            sig,
            "the function is async, const, unsafe or extern",
            fix,
        ));
        ok = false;
    }
    if sig
        .generics
        .params
        .iter()
        .any(|p| !matches!(p, GenericParam::Lifetime(_)))
    {
        errs.push(error(
            "Z0103",
            &sig.generics,
            "the function has type or const generics",
            fix,
        ));
        ok = false;
    }
    let mut inputs = sig.inputs.iter();
    let ctx_mut = match inputs.next() {
        Some(syn::FnArg::Typed(t)) => match &*t.ty {
            Type::Reference(r)
                if matches!(&*r.elem, Type::Path(p) if p.qself.is_none()
                    && last_ident(&p.path).is_some_and(|i| i == "Ctx")) =>
            {
                Some(r.mutability.is_some())
            }
            _ => None,
        },
        _ => None,
    };
    let Some(ctx_mut) = ctx_mut else {
        errs.push(error(
            "Z0103",
            &sig.inputs,
            "the first parameter is not `&Ctx` or `&mut Ctx`",
            fix,
        ));
        return None;
    };
    let mut args = Vec::new();
    for input in inputs {
        match input {
            syn::FnArg::Receiver(r) => {
                errs.push(error("Z0103", r, "the function has a `self` receiver", fix));
                ok = false;
            }
            syn::FnArg::Typed(t) => match &*t.pat {
                Pat::Ident(p) if p.subpat.is_none() && p.by_ref.is_none() => {
                    args.push((p.ident.unraw().to_string(), (*t.ty).clone()));
                }
                other => {
                    errs.push(error(
                        "Z0103",
                        other,
                        "the parameter is not a plain name",
                        fix,
                    ));
                    ok = false;
                }
            },
        }
    }
    ok.then_some((ctx_mut, args))
}

/// Reads a return type into its success type and whether it is a
/// `Result` over the module's error enum.
fn parse_return(
    output: &ReturnType,
    error_ident: Option<&Ident>,
    errs: &mut Errors,
) -> Option<(Type, bool)> {
    let unit: Type = syn::parse_quote!(());
    let ReturnType::Type(_, ty) = output else {
        return Some((unit, false));
    };
    let Type::Path(p) = &**ty else {
        return Some(((**ty).clone(), false));
    };
    let Some(seg) = p
        .path
        .segments
        .last()
        .filter(|s| s.ident == "Result" && p.qself.is_none())
    else {
        return Some(((**ty).clone(), false));
    };
    let fix = "return the module's error enum";
    let types: Vec<&Type> = match &seg.arguments {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|a| match a {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    if types.len() != 2 {
        errs.push(error(
            "Z0105",
            ty,
            "a `Result` without two type arguments",
            fix,
        ));
        return None;
    }
    let matches_enum = matches!(types[1], Type::Path(e)
        if e.qself.is_none() && last_ident(&e.path).is_some_and(|i| Some(i) == error_ident));
    if !matches_enum {
        errs.push(error(
            "Z0105",
            types[1],
            "the error type is not the module's `#[contract_error]` enum",
            fix,
        ));
        return None;
    }
    Some((types[0].clone(), true))
}

fn parse_method(
    f: &mut syn::ItemFn,
    attr: &Attribute,
    error_ident: Option<&Ident>,
    errs: &mut Errors,
) -> Option<Method> {
    let mut payable = false;
    let mut returns = Vec::new();
    let mut bad_args = false;
    if matches!(attr.meta, Meta::List(_)) {
        let parsed = attr.parse_nested_meta(|m| {
            if m.path.is_ident("payable") {
                payable = true;
                Ok(())
            } else if m.path.is_ident("returns") {
                m.parse_nested_meta(|r| {
                    let id = r
                        .path
                        .get_ident()
                        .ok_or_else(|| r.error("expected a name"))?;
                    returns.push(id.unraw().to_string());
                    Ok(())
                })
            } else {
                Err(m.error("unknown argument"))
            }
        });
        if let Err(e) = parsed {
            errs.push(syn::Error::new(
                e.span(),
                "Z0107: unknown or malformed `#[method]` argument\nfix: use `payable` or `returns(name, ...)`",
            ));
            bad_args = true;
        }
    } else if !matches!(attr.meta, Meta::Path(_)) {
        errs.push(error(
            "Z0107",
            attr,
            "malformed `#[method]` argument",
            "use `payable` or `returns(name, ...)`",
        ));
        bad_args = true;
    }

    let sig = parse_signature(&f.sig, errs);
    let ret = parse_return(&f.sig.output, error_ident, errs);
    let (ctx_mut, args) = sig?;
    let (ok, fallible) = ret?;
    if payable && !ctx_mut {
        errs.push(error(
            "Z0104",
            &f.sig.inputs,
            "a `payable` method takes `&Ctx`",
            "take `&mut Ctx`, or drop `payable`",
        ));
        return None;
    }
    if bad_args {
        return None;
    }
    let name = f.sig.ident.unraw().to_string();
    Some(Method {
        cfgs: cfgs(&f.attrs),
        ident: f.sig.ident.clone(),
        selector: selector(&name),
        name,
        payable,
        returns,
        ctx_mut,
        args,
        ok,
        fallible,
        doc: doc(&f.attrs),
    })
}

fn parse_handler(
    f: &mut syn::ItemFn,
    attr: &Attribute,
    error_ident: Option<&Ident>,
    errs: &mut Errors,
) -> Option<Handler> {
    if !matches!(attr.meta, Meta::Path(_)) {
        errs.push(error(
            "Z0107",
            attr,
            "`#[on_deposit]` takes no arguments",
            "use `#[on_deposit]` alone",
        ));
        return None;
    }
    let sig = parse_signature(&f.sig, errs);
    let ret = parse_return(&f.sig.output, error_ident, errs);
    let (ctx_mut, args) = sig?;
    let (ok, fallible) = ret?;
    if !args.is_empty() || !is_unit(&ok) {
        errs.push(error(
            "Z0121",
            &f.sig,
            "the handler takes arguments after the ctx or returns more than `()`",
            "keep one handler taking only `ctx`",
        ));
        return None;
    }
    Some(Handler {
        cfgs: cfgs(&f.attrs),
        ident: f.sig.ident.clone(),
        ctx_mut,
        fallible,
    })
}

fn parse_event(
    s: &mut syn::ItemStruct,
    attr: &Attribute,
    earlier: &[Event],
    errs: &mut Errors,
) -> Option<Event> {
    let fix = "use a named-field struct with a unique name";
    if !matches!(attr.meta, Meta::Path(_)) {
        errs.push(error(
            "Z0107",
            attr,
            "`#[event]` takes no arguments",
            "use `#[event]` alone",
        ));
        return None;
    }
    let mut ok = true;
    if !s.generics.params.is_empty() {
        errs.push(error("Z0115", &s.generics, "the event has generics", fix));
        ok = false;
    }
    if earlier
        .iter()
        .any(|e| e.ident == s.ident && e.cfgs.is_empty())
        && cfgs(&s.attrs).is_empty()
    {
        errs.push(error(
            "Z0115",
            &s.ident,
            "an event with this name exists",
            fix,
        ));
        ok = false;
    }
    let Fields::Named(named) = &mut s.fields else {
        errs.push(error(
            "Z0115",
            &s.ident,
            "the event is not a named-field struct",
            fix,
        ));
        return None;
    };
    let mut fields = Vec::new();
    let mut indexed_count = 0;
    for f in named.named.iter_mut() {
        let before = f.attrs.len();
        f.attrs.retain(|a| !a.path().is_ident("indexed"));
        let indexed = f.attrs.len() != before;
        if indexed {
            indexed_count += 1;
            if indexed_count == 4 {
                errs.push(error(
                    "Z0113",
                    &f.ident,
                    "more than 3 indexed fields",
                    "index at most 3 fields",
                ));
                ok = false;
            }
        }
        fields.push((f.ident.clone().expect("named field"), f.ty.clone(), indexed));
    }
    ok.then(|| Event {
        cfgs: cfgs(&s.attrs),
        ident: s.ident.clone(),
        fields,
        doc: doc(&s.attrs),
    })
}

/// The arguments of `#[storage(0xNN)]` and
/// `#[storage(0xNN, key(a, b))]`.
fn storage_args(attr: &Attribute, errs: &mut Errors) -> Option<(u8, Option<Vec<String>>)> {
    let Meta::List(_) = &attr.meta else {
        errs.push(error(
            "Z0116",
            attr,
            "`#[storage]` has no namespace",
            "use a literal from `0x01` to `0xff`",
        ));
        return None;
    };
    let parsed = attr.parse_args_with(|input: ParseStream| {
        let ns: Expr = input.parse()?;
        let mut key = None;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let id: Ident = input.parse()?;
            if id != "key" {
                return Err(syn::Error::new(id.span(), "expected `key(...)`"));
            }
            let content;
            parenthesized!(content in input);
            let names = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?;
            key = Some(
                names
                    .iter()
                    .map(|i| i.unraw().to_string())
                    .collect::<Vec<_>>(),
            );
        }
        Ok((ns, key))
    });
    let (ns, key) = match parsed {
        Ok(v) => v,
        Err(e) => {
            errs.push(syn::Error::new(
                e.span(),
                "Z0107: malformed `#[storage]` argument\nfix: use `#[storage(0xNN)]` or `#[storage(0xNN, key(a, b))]`",
            ));
            return None;
        }
    };
    let value = match &ns {
        Expr::Lit(l) => match &l.lit {
            Lit::Int(i) => i.base10_parse::<u8>().ok().filter(|n| *n != 0),
            _ => None,
        },
        _ => None,
    };
    match value {
        Some(n) => Some((n, key)),
        None => {
            errs.push(error(
                "Z0116",
                &ns,
                "the namespace is not an integer literal in 0x01..=0xff",
                "use a literal from `0x01` to `0xff`",
            ));
            None
        }
    }
}

fn parse_storage(c: &mut syn::ItemConst, attr: &Attribute, errs: &mut Errors) -> Option<Storage> {
    let args = storage_args(attr, errs);
    let fix = "declare `const NAME: Item<V> = Item::new();`";
    let Some((kind, types)) = storage_type(&c.ty) else {
        errs.push(error(
            "Z0120",
            &c.ty,
            "the type is not `Item<V>` or `Map<K, V>`",
            fix,
        ));
        return None;
    };
    let is_map = kind == "Map";
    let want = if is_map { 2 } else { 1 };
    if types.len() != want {
        errs.push(error(
            "Z0120",
            &c.ty,
            "the type has the wrong number of arguments",
            fix,
        ));
        return None;
    }
    let (namespace, key_names) = args?;
    let key_fix = "name one key field per tuple element";
    if !is_map && key_names.is_some() {
        errs.push(error("Z0118", attr, "`key(...)` on an `Item`", key_fix));
        return None;
    }
    if is_map && key_names.is_none() {
        errs.push(error("Z0118", attr, "a `Map` without `key(...)`", key_fix));
        return None;
    }
    let key = is_map.then(|| types[0].clone());
    let value = (*types.last().expect("one or two arguments")).clone();
    let ns = proc_macro2::Literal::u8_suffixed(namespace);
    let ctor = if is_map {
        quote!(::zeno_contract::Map::__with_namespace(#ns))
    } else {
        quote!(::zeno_contract::Item::__with_namespace(#ns))
    };
    *c.expr = syn::parse2::<Expr>(ctor).expect("a call expression");
    Some(Storage {
        cfgs: cfgs(&c.attrs),
        ident: c.ident.clone(),
        name: c.ident.to_string().to_lowercase(),
        namespace,
        key_names: key_names.unwrap_or_default(),
        key,
        value,
        doc: doc(&c.attrs),
    })
}
