//! Internal proc-macro crate for `dynoffsets`.
//!
//! Do not use directly — depend on `dynoffsets` instead.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    ext::IdentExt,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, Ident, Item, ItemConst, ItemMod, LitBool, LitStr, Token,
};

struct Args {
    dll: String,
    enabled: bool,
    /// True when `r#static` was passed; use `AtomicUsize` cells + register fn.
    static_storage: bool,
    /// True when `hashed` was passed; emit fnv1a-wrapped lookup to avoid &str in .rdata.
    hashed: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            dll: "client.dll".to_string(),
            enabled: true,
            static_storage: false,
            hashed: false,
        }
    }
}

enum Arg {
    Dll(LitStr),
    Enabled(LitBool),
    Static,
    Hashed,
}

impl Parse for Arg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            return Ok(Arg::Dll(input.parse()?));
        }
        if input.peek(LitBool) {
            return Ok(Arg::Enabled(input.parse()?));
        }
        if input.peek(Token![static]) {
            let _: Token![static] = input.parse()?;
            return Ok(Arg::Static);
        }
        if input.peek(Ident::peek_any) {
            let id: Ident = input.call(Ident::parse_any)?;
            let s = id.to_string();
            if matches!(s.as_str(), "static" | "r#static") {
                return Ok(Arg::Static);
            }
            if matches!(s.as_str(), "hashed" | "hash" | "h") {
                return Ok(Arg::Hashed);
            }
            return Err(syn::Error::new(
                id.span(),
                "unknown flag; expected `r#static`, `hashed`, a dll name string, or a bool",
            ));
        }
        Err(input.error("expected dll name string, bool, `r#static`, or `hashed`"))
    }
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = Args::default();
        if input.is_empty() {
            return Ok(args);
        }
        let list: Punctuated<Arg, Token![,]> = Punctuated::parse_terminated(input)?;
        for arg in list {
            match arg {
                Arg::Dll(s) => args.dll = s.value(),
                Arg::Enabled(b) => args.enabled = b.value,
                Arg::Static => args.static_storage = true,
                Arg::Hashed => args.hashed = true,
            }
        }
        Ok(args)
    }
}

fn slot_ident(name: &Ident) -> Ident {
    format_ident!("__DYNOFFSETS_{}", name, span = name.span())
}

struct ConstInfo {
    fn_name: Ident,
    vis: syn::Visibility,
    lit_expr: TokenStream2,
    name_str: LitStr,
}

impl ConstInfo {
    fn from_item_const(c: &ItemConst) -> Self {
        let fn_name = c.ident.clone();
        let name_str = LitStr::new(&fn_name.to_string(), c.ident.span());
        Self {
            fn_name,
            vis: c.vis.clone(),
            lit_expr: expr_tokens(&c.expr),
            name_str,
        }
    }
}

fn process_static_const(c: &ItemConst) -> Option<(TokenStream2, Item, Item)> {
    if !is_pub_usize(c) {
        return None;
    }
    let info = ConstInfo::from_item_const(c);
    let slot = slot_ident(&info.fn_name);
    let name_str = &info.name_str;
    let entry = quote! { (#name_str, &#slot) };
    let stat = parse_slot_static(&slot, &info.lit_expr);
    let fun = parse_slot_fn(&info.vis, &info.fn_name, &slot);
    Some((entry, stat, fun))
}

fn rewrite_dynamic_consts(
    items: &mut [Item],
    mut build_fn: impl FnMut(&ConstInfo) -> TokenStream2,
) {
    for item in items.iter_mut() {
        let Item::Const(c) = item else { continue };
        if !is_pub_usize(c) {
            continue;
        }
        let info = ConstInfo::from_item_const(c);
        *item = syn::parse2(build_fn(&info)).expect("fn");
    }
}

fn rewrite_static_module(
    items: &mut Vec<Item>,
    build_register: impl FnOnce(&[TokenStream2]) -> TokenStream2,
) {
    let mut entries: Vec<TokenStream2> = Vec::new();
    let mut new_items: Vec<Item> = Vec::with_capacity(items.len() * 2 + 1);
    for item in items.iter() {
        if let Item::Const(c) = item {
            if let Some((entry, slot, accessor)) = process_static_const(c) {
                entries.push(entry);
                new_items.push(slot);
                new_items.push(accessor);
                continue;
            }
        }
        new_items.push(item.clone());
    }
    new_items.push(syn::parse2(build_register(&entries)).expect("register fn"));
    *items = new_items;
}

#[proc_macro_attribute]
pub fn schema(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as Args);
    let mut module = parse_macro_input!(item as ItemMod);
    rewrite_schema_module(&mut module, &args);
    quote!(#module).into()
}

fn rewrite_schema_module(class_mod: &mut ItemMod, args: &Args) {
    let Some((_, items)) = class_mod.content.as_mut() else {
        return;
    };
    let class_lit = LitStr::new(&class_mod.ident.to_string(), class_mod.ident.span());
    let dll = &args.dll;

    if args.static_storage {
        rewrite_static_module(items, |entries| {
            quote! {
                /// Register slots for `populate()`. Call after `init`.
                pub fn __dynoffsets_register() {
                    ::dynoffsets::__register_schema_static(#dll, #class_lit, &[#(#entries),*]);
                }
            }
        });
        return;
    }

    let enabled = args.enabled;
    let hashed = args.hashed;
    rewrite_dynamic_consts(items, |info| {
        let ConstInfo {
            fn_name,
            vis,
            lit_expr,
            name_str: field_str,
        } = info;
        if enabled {
            let lookup = if hashed {
                let dll_len = dll.len() as u16;
                let class_len = class_lit.value().len() as u16;
                let field_len = field_str.value().len() as u16;
                quote! {
                    ::dynoffsets::lookup_or_fallback_h(
                        ::dynoffsets::fnv1a(#dll), #dll_len,
                        ::dynoffsets::fnv1a(#class_lit), #class_len,
                        ::dynoffsets::fnv1a(#field_str), #field_len,
                        #lit_expr
                    )
                }
            } else {
                quote! {
                    ::dynoffsets::lookup_or_fallback(#dll, #class_lit, #field_str, #lit_expr)
                }
            };
            cached_accessor(vis, fn_name, lit_expr, lookup)
        } else {
            quote! {
                // code generated by https://github.com/H0llyW00dzZ/dynoffsets — do not edit
                #vis fn #fn_name() -> usize { #lit_expr }
            }
        }
    });
}

#[proc_macro_attribute]
pub fn globals(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as Args);
    let mut module = parse_macro_input!(item as ItemMod);
    rewrite_globals_module(&mut module, &args);
    quote!(#module).into()
}

fn rewrite_globals_module(module: &mut ItemMod, args: &Args) {
    let Some((_, items)) = module.content.as_mut() else {
        return;
    };

    if args.static_storage {
        rewrite_static_module(items, |entries| {
            quote! {
                /// Register slots for `populate()`. Call after `init`.
                pub fn __dynoffsets_register() {
                    ::dynoffsets::__register_globals_static(&[#(#entries),*]);
                }
            }
        });
        return;
    }

    rewrite_dynamic_consts(items, |info| {
        let ConstInfo {
            fn_name,
            vis,
            lit_expr,
            ..
        } = info;
        cached_accessor(
            vis,
            fn_name,
            lit_expr,
            quote! {
                ::dynoffsets::get_runtime_globals()
                    .and_then(|g| g.#fn_name)
                    .unwrap_or(#lit_expr)
            },
        )
    });
}

#[proc_macro_attribute]
pub fn interfaces(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as Args);
    let mut module = parse_macro_input!(item as ItemMod);
    rewrite_interfaces_module(&mut module, &args);
    quote!(#module).into()
}

fn rewrite_interfaces_module(module: &mut ItemMod, args: &Args) {
    let Some((_, items)) = module.content.as_mut() else {
        return;
    };
    let dll = &args.dll;

    if args.static_storage {
        rewrite_static_module(items, |entries| {
            quote! {
                /// Register slots for `populate()`. Call after `init`.
                pub fn __dynoffsets_register() {
                    ::dynoffsets::__register_interfaces_static(#dll, &[#(#entries),*]);
                }
            }
        });
        return;
    }

    let enabled = args.enabled;
    rewrite_dynamic_consts(items, |info| {
        let ConstInfo {
            fn_name,
            vis,
            lit_expr,
            name_str,
        } = info;
        if enabled {
            cached_accessor(
                vis,
                fn_name,
                lit_expr,
                quote! {
                    ::dynoffsets::get_runtime_interfaces()
                        .and_then(|i| i.get(#dll, #name_str))
                        .unwrap_or(#lit_expr)
                },
            )
        } else {
            quote! {
                // code generated by https://github.com/H0llyW00dzZ/dynoffsets — do not edit
                #[inline] #vis fn #fn_name() -> usize { #lit_expr }
            }
        }
    });
}

#[proc_macro_attribute]
pub fn buttons(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as Args);
    let mut module = parse_macro_input!(item as ItemMod);
    rewrite_buttons_module(&mut module, &args);
    quote!(#module).into()
}

fn rewrite_buttons_module(module: &mut ItemMod, args: &Args) {
    let Some((_, items)) = module.content.as_mut() else {
        return;
    };

    if args.static_storage {
        rewrite_static_module(items, |entries| {
            quote! {
                /// Register slots for `populate()`. Call after `init`.
                pub fn __dynoffsets_register() {
                    ::dynoffsets::__register_buttons_static(&[#(#entries),*]);
                }
            }
        });
        return;
    }

    rewrite_dynamic_consts(items, |info| {
        let ConstInfo {
            fn_name,
            vis,
            lit_expr,
            name_str,
        } = info;
        cached_accessor(
            vis,
            fn_name,
            lit_expr,
            quote! {
                ::dynoffsets::get_runtime_buttons()
                    .and_then(|b| b.get(#name_str))
                    .unwrap_or(#lit_expr)
            },
        )
    });
}

/// Emits a `#[inline]` accessor whose first post-`init` invocation resolves
/// `lookup_expr` (a `usize` expression that already folds the literal fallback
/// in via `unwrap_or`), latches the result into a per-accessor `OnceCell<usize>`,
/// and from then on returns it via a single cheap atomic check + pointer load.
///
/// Pre-`init` calls deliberately *do not* write the cell — they just return
/// `lit_expr` — so an early call before [`is_initialized`] never latches the
/// literal fallback. The first post-`init` call wins the `OnceCell` and from
/// then on every other call is a fast `OnceCell::get()` hit, regardless of
/// whether the live value was found or the literal was used (so a pattern miss
/// no longer pays the slow path on every frame).
fn cached_accessor(
    vis: &syn::Visibility,
    fn_name: &Ident,
    lit_expr: &TokenStream2,
    lookup_expr: TokenStream2,
) -> TokenStream2 {
    quote! {
        // code generated by https://github.com/H0llyW00dzZ/dynoffsets — do not edit
        #[inline]
        #vis fn #fn_name() -> usize {
            static CELL: ::dynoffsets::__AccessorCell<usize> =
                ::dynoffsets::__AccessorCell::new();

            if let ::core::option::Option::Some(&v) = CELL.get() {
                return v;
            }

            #[cold]
            #[inline(never)]
            fn __dynoffsets_resolve() -> usize {
                if !::dynoffsets::is_initialized() {
                    return #lit_expr;
                }
                *CELL.get_or_init(|| { let v: usize = #lookup_expr; v })
            }

            __dynoffsets_resolve()
        }
    }
}

fn parse_slot_static(slot: &Ident, lit_expr: &TokenStream2) -> Item {
    syn::parse2(quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub static #slot: ::core::sync::atomic::AtomicUsize =
            ::core::sync::atomic::AtomicUsize::new(#lit_expr);
    })
    .expect("slot static")
}

fn parse_slot_fn(vis: &syn::Visibility, fn_name: &Ident, slot: &Ident) -> Item {
    syn::parse2(quote! {
        #[inline]
        #vis fn #fn_name() -> usize {
            #slot.load(::core::sync::atomic::Ordering::Relaxed)
        }
    })
    .expect("slot fn")
}

fn is_pub_usize(c: &ItemConst) -> bool {
    matches!(c.vis, syn::Visibility::Public(_))
        && matches!(c.ty.as_ref(), syn::Type::Path(p) if p.path.is_ident("usize"))
}

fn expr_tokens(expr: &Expr) -> TokenStream2 {
    quote!(#expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_default_is_client_dll_enabled() {
        let a = Args::default();
        assert_eq!(a.dll, "client.dll");
        assert!(a.enabled);
    }

    #[test]
    fn args_parse_empty_uses_defaults() {
        let a: Args = syn::parse_str("").unwrap();
        assert_eq!(a.dll, "client.dll");
        assert!(a.enabled);
    }

    #[test]
    fn args_parse_dll_string() {
        let a: Args = syn::parse_str("\"server.dll\"").unwrap();
        assert_eq!(a.dll, "server.dll");
        assert!(a.enabled);
    }

    #[test]
    fn args_parse_bool_only() {
        let a: Args = syn::parse_str("false").unwrap();
        assert!(!a.enabled);
    }

    #[test]
    fn args_parse_dll_and_bool() {
        let a: Args = syn::parse_str("\"engine2.dll\", false").unwrap();
        assert_eq!(a.dll, "engine2.dll");
        assert!(!a.enabled);
    }

    #[test]
    fn args_parse_two_dlls_last_wins() {
        let a: Args = syn::parse_str("\"a.dll\", \"b.dll\"").unwrap();
        assert_eq!(a.dll, "b.dll");
    }

    #[test]
    fn args_parse_invalid_token_errors() {
        let r: syn::Result<Args> = syn::parse_str("123");
        assert!(r.is_err());
    }

    #[test]
    fn args_parse_missing_comma_errors() {
        let r: syn::Result<Args> = syn::parse_str("\"a.dll\" false");
        assert!(r.is_err());
    }

    #[test]
    fn is_pub_usize_accepts_pub_usize_const() {
        let c: ItemConst = syn::parse_str("pub const X: usize = 1;").unwrap();
        assert!(is_pub_usize(&c));
    }

    #[test]
    fn is_pub_usize_rejects_private_const() {
        let c: ItemConst = syn::parse_str("const X: usize = 1;").unwrap();
        assert!(!is_pub_usize(&c));
    }

    #[test]
    fn is_pub_usize_rejects_non_usize_const() {
        let c: ItemConst = syn::parse_str("pub const X: u32 = 1;").unwrap();
        assert!(!is_pub_usize(&c));
    }

    #[test]
    fn is_pub_usize_rejects_complex_type() {
        let c: ItemConst = syn::parse_str("pub const X: [u8; 4] = [0; 4];").unwrap();
        assert!(!is_pub_usize(&c));
    }

    #[test]
    fn expr_tokens_roundtrip_simple() {
        let e: Expr = syn::parse_str("42").unwrap();
        assert_eq!(expr_tokens(&e).to_string(), "42");
    }

    #[test]
    fn expr_tokens_roundtrip_arith() {
        let e: Expr = syn::parse_str("0x10 + 0x08").unwrap();
        assert!(!expr_tokens(&e).to_string().is_empty());
    }

    fn parse_mod(src: &str) -> ItemMod {
        syn::parse_str(src).unwrap()
    }

    #[test]
    fn rewrite_schema_enabled_emits_lookup_call() {
        let mut m = parse_mod("mod C_Foo { pub const m_x: usize = 0x10; }");
        rewrite_schema_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert!(s.contains("lookup_or_fallback"));
        assert!(s.contains("\"C_Foo\""));
        assert!(s.contains("\"m_x\""));
        // Per-accessor `OnceCell<usize>` cache machinery is in place.
        assert!(s.contains("__AccessorCell"));
        assert!(s.contains("is_initialized"));
        assert!(s.contains("get_or_init"));
    }

    #[test]
    fn rewrite_schema_disabled_emits_literal_fn() {
        let mut m = parse_mod("mod C_Foo { pub const m_x: usize = 0x10; }");
        let args = Args {
            dll: "client.dll".into(),
            enabled: false,
            static_storage: false,
            hashed: false,
        };
        rewrite_schema_module(&mut m, &args);
        let s = quote!(#m).to_string();
        assert!(!s.contains("lookup_or_fallback"));
        assert!(!s.contains("__AccessorCell"));
        assert!(s.contains("fn m_x"));
    }

    #[test]
    fn rewrite_schema_skips_non_pub_usize_items() {
        let mut m = parse_mod(
            "mod C_Foo { const priv_x: usize = 1; pub const non_usize: u32 = 2; \
             pub fn f() {} pub const ok: usize = 0x10; }",
        );
        rewrite_schema_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        // exactly one rewritten item (the pub const usize one)
        let count = s.matches("lookup_or_fallback").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn rewrite_schema_hashed_emits_fnv_no_strings() {
        let mut m = parse_mod("mod C_Foo { pub const m_x: usize = 0x10; }");
        let mut a = Args::default();
        a.hashed = true;
        rewrite_schema_module(&mut m, &a);
        let s = quote!(#m).to_string();
        assert!(s.contains("lookup_or_fallback_h"));
        assert!(s.contains("fnv1a"));
        // the str version should not be called for this accessor
        assert_eq!(s.matches("lookup_or_fallback(").count(), 0);
        // `u16` length literals next to each `fnv1a(...)` call: "client.dll" -> 10,
        // "C_Foo" -> 5, "m_x" -> 3.
        assert!(s.contains("10u16"));
        assert!(s.contains("5u16"));
        assert!(s.contains("3u16"));
    }

    #[test]
    fn rewrite_schema_handles_module_without_body() {
        // `mod E;` has no inline content → function returns early without touching anything.
        let mut m = parse_mod("mod E;");
        rewrite_schema_module(&mut m, &Args::default());
        rewrite_globals_module(&mut m, &Args::default());
        rewrite_interfaces_module(&mut m, &Args::default());
        rewrite_buttons_module(&mut m, &Args::default());
    }

    #[test]
    fn rewrite_globals_emits_runtime_lookup() {
        let mut m = parse_mod("mod g { pub const dw_thing: usize = 0x42; }");
        rewrite_globals_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert!(s.contains("get_runtime_globals"));
        assert!(s.contains("dw_thing"));
        // Per-accessor `OnceCell<usize>` cache machinery is in place.
        assert!(s.contains("__AccessorCell"));
        assert!(s.contains("is_initialized"));
        assert!(s.contains("get_or_init"));
    }

    #[test]
    fn rewrite_interfaces_enabled_emits_lookup() {
        let mut m = parse_mod("mod i { pub const Source2Client002: usize = 0xAA; }");
        rewrite_interfaces_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert!(s.contains("get_runtime_interfaces"));
        assert!(s.contains("\"Source2Client002\""));
        // Per-accessor `OnceCell<usize>` cache machinery is in place.
        assert!(s.contains("__AccessorCell"));
        assert!(s.contains("is_initialized"));
        assert!(s.contains("get_or_init"));
    }

    #[test]
    fn rewrite_interfaces_disabled_emits_literal() {
        let mut m = parse_mod("mod i { pub const Source2Client002: usize = 0xAA; }");
        let args = Args {
            dll: "client.dll".into(),
            enabled: false,
            static_storage: false,
            hashed: false,
        };
        rewrite_interfaces_module(&mut m, &args);
        let s = quote!(#m).to_string();
        assert!(!s.contains("get_runtime_interfaces"));
        assert!(!s.contains("__AccessorCell"));
    }

    #[test]
    fn rewrite_buttons_emits_runtime_lookup() {
        let mut m = parse_mod("mod b { pub const in_attack: usize = 0x100; }");
        rewrite_buttons_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert!(s.contains("get_runtime_buttons"));
        assert!(s.contains("\"in_attack\""));
        // Per-accessor `OnceCell<usize>` cache machinery is in place.
        assert!(s.contains("__AccessorCell"));
        assert!(s.contains("is_initialized"));
        assert!(s.contains("get_or_init"));
    }

    #[test]
    fn rewrite_globals_skips_non_pub_usize_items() {
        let mut m = parse_mod(
            "mod g { const priv_x: usize = 1; pub const ok: usize = 2; pub const non_usize: u32 = 3; }",
        );
        rewrite_globals_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert_eq!(s.matches("get_runtime_globals").count(), 1);
    }

    #[test]
    fn rewrite_interfaces_skips_non_pub_usize_items() {
        let mut m = parse_mod(
            "mod i { const priv_x: usize = 1; pub const ok: usize = 2; pub const non_usize: u32 = 3; }",
        );
        rewrite_interfaces_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert_eq!(s.matches("get_runtime_interfaces").count(), 1);
    }

    #[test]
    fn rewrite_buttons_skips_non_pub_usize_items() {
        let mut m = parse_mod(
            "mod b { const priv_x: usize = 1; pub const ok: usize = 2; pub const non_usize: u32 = 3; }",
        );
        rewrite_buttons_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert_eq!(s.matches("get_runtime_buttons").count(), 1);
    }

    #[test]
    fn rewrite_modules_with_empty_body_are_unchanged() {
        let mut m = parse_mod("mod empty {}");
        rewrite_schema_module(&mut m, &Args::default());
        rewrite_globals_module(&mut m, &Args::default());
        rewrite_interfaces_module(&mut m, &Args::default());
        rewrite_buttons_module(&mut m, &Args::default());
        let s = quote!(#m).to_string();
        assert!(s.contains("mod empty"));
    }

    fn static_args(dll: &str) -> Args {
        Args {
            dll: dll.into(),
            enabled: true,
            static_storage: true,
            hashed: false,
        }
    }

    #[test]
    fn args_parse_raw_static_keyword() {
        let a: Args = syn::parse_str("r#static").unwrap();
        assert!(a.static_storage);
        assert_eq!(a.dll, "client.dll");
        assert!(a.enabled);
    }

    #[test]
    fn args_parse_bare_static_keyword() {
        // Even though `static` alone isn't writable in user attribute source
        // (rustc would treat it as the keyword before reaching the macro),
        // the parser still accepts the keyword token directly.
        let a: Args = syn::parse_str("static").unwrap();
        assert!(a.static_storage);
    }

    #[test]
    fn args_parse_dll_then_static() {
        let a: Args = syn::parse_str("\"server.dll\", r#static").unwrap();
        assert_eq!(a.dll, "server.dll");
        assert!(a.static_storage);
        assert!(a.enabled);
    }

    #[test]
    fn args_parse_unknown_ident_errors() {
        let r: syn::Result<Args> = syn::parse_str("bogus");
        assert!(r.is_err());
    }

    #[test]
    fn rewrite_schema_static_mode_emits_atomic_storage_and_register() {
        let mut m = parse_mod("mod C_Foo { pub const m_x: usize = 0x10; }");
        rewrite_schema_module(&mut m, &static_args("client.dll"));
        let s = quote!(#m).to_string();
        assert!(s.contains("AtomicUsize"));
        assert!(s.contains("__DYNOFFSETS_m_x"));
        assert!(s.contains("__register_schema_static"));
        assert!(s.contains("\"client.dll\""));
        assert!(s.contains("\"C_Foo\""));
        assert!(s.contains("\"m_x\""));
        assert!(s.contains("fn __dynoffsets_register"));
        // The accessor body must not contain any library call or the dynamic
        // OnceCell cache (static mode uses its own AtomicUsize slots).
        assert!(!s.contains("lookup_or_fallback"));
        assert!(!s.contains("__AccessorCell"));
    }

    #[test]
    fn rewrite_globals_static_mode_emits_atomic_storage_and_register() {
        let mut m = parse_mod("mod g { pub const dw_thing: usize = 0x42; }");
        rewrite_globals_module(&mut m, &static_args("client.dll"));
        let s = quote!(#m).to_string();
        assert!(s.contains("AtomicUsize"));
        assert!(s.contains("__DYNOFFSETS_dw_thing"));
        assert!(s.contains("__register_globals_static"));
        assert!(s.contains("fn __dynoffsets_register"));
        // Accessor must not contain the dynamic lookup.
        assert!(!s.contains("get_runtime_globals"));
    }

    #[test]
    fn rewrite_interfaces_static_mode_emits_atomic_storage_and_register() {
        let mut m = parse_mod("mod i { pub const Source2Client002: usize = 0xAA; }");
        rewrite_interfaces_module(&mut m, &static_args("server.dll"));
        let s = quote!(#m).to_string();
        assert!(s.contains("AtomicUsize"));
        assert!(s.contains("__DYNOFFSETS_Source2Client002"));
        assert!(s.contains("__register_interfaces_static"));
        assert!(s.contains("\"server.dll\""));
        assert!(s.contains("\"Source2Client002\""));
        assert!(!s.contains("get_runtime_interfaces"));
    }

    #[test]
    fn rewrite_buttons_static_mode_emits_atomic_storage_and_register() {
        let mut m = parse_mod("mod b { pub const in_attack: usize = 0x100; }");
        rewrite_buttons_module(&mut m, &static_args("client.dll"));
        let s = quote!(#m).to_string();
        assert!(s.contains("AtomicUsize"));
        assert!(s.contains("__DYNOFFSETS_in_attack"));
        assert!(s.contains("__register_buttons_static"));
        assert!(s.contains("\"in_attack\""));
        assert!(!s.contains("get_runtime_buttons"));
    }

    #[test]
    fn static_mode_skips_non_pub_usize_items() {
        let mut m = parse_mod(
            "mod g { const priv_x: usize = 1; pub const ok: usize = 2; pub const non_usize: u32 = 3; }",
        );
        rewrite_globals_module(&mut m, &static_args("client.dll"));
        let s = quote!(#m).to_string();
        // Exactly one offset was rewritten into a storage cell.
        assert_eq!(s.matches("AtomicUsize :: new").count(), 1);
        // The slot ident is referenced 3 times: static decl, accessor load, register entry.
        assert_eq!(s.matches("__DYNOFFSETS_ok").count(), 3);
        // Non-pub / non-usize consts are preserved unchanged.
        assert!(s.contains("const priv_x"));
        assert!(s.contains("const non_usize"));
    }

    #[test]
    fn slot_ident_prefixes_input() {
        let id = syn::Ident::new("m_iHealth", proc_macro2::Span::call_site());
        assert_eq!(slot_ident(&id).to_string(), "__DYNOFFSETS_m_iHealth");
    }
}
