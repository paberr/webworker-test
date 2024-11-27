#![deny(rustdoc::broken_intra_doc_links, missing_docs)]

//! A crate to export a function to the webworker.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

/// A procedural macro that exports a function for use with a webworker.
#[proc_macro_attribute]
pub fn webworker_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let fn_name = &input.sig.ident;

    // Generate a module with the wrapper function
    let wrapper_fn_name = format_ident!("__webworker_{}", fn_name);
    let mod_code = quote! {
        pub mod #fn_name {
            pub const __WEBWORKER: () = ();
            const _: () = {
                #[wasm_bindgen::prelude::wasm_bindgen]
                pub fn #wrapper_fn_name(arg: Box<[u8]>) -> Box<[u8]> {
                    let arg = webworker::convert::from_bytes(&arg);
                    let res = super::#fn_name(arg);
                    let res = webworker::convert::to_bytes(&res);
                    res
                }
            };
        }
    };

    // Combine everything into the final output
    let expanded = quote! {
        #input

        #mod_code
    };

    TokenStream::from(expanded)
}
