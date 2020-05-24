extern crate proc_macro;

use proc_macro::*;
use quote::{quote_spanned, quote};
use syn::{Ident};
use syn::spanned::Spanned;

struct Route {
    name: Ident,
    ast: syn::ItemFn,
}

impl Route {
    pub fn new(
        input: TokenStream,
    ) -> syn::Result<Self> {
        let ast: syn::ItemFn = syn::parse(input)?;
        let name = ast.sig.ident.clone();

        Ok(Self {
            name,
            ast,
        })
    }
}

#[proc_macro_derive(TaskSet)]
pub fn taskset_derive(input: TokenStream) -> TokenStream {
    let ast = syn::parse(input).unwrap();
    impl_taskset(&ast)

}

fn impl_taskset(ast: &syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let gen = quote! {
        impl TaskSet for #name {
        }
    };
    gen.into()
}

#[proc_macro_attribute]
pub fn task(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let gen = match Route::new(item) {
        Ok(gen) => gen,
        Err(err) => return err.to_compile_error().into(),
    };
    // @TODO: actually use this, for now just mask an error
    let _ = gen.ast;
    let x = format!(r#"
        const {}: bool = true;
    "#, gen.name);
    x.parse().expect("Generated invalid tokens")
}


// Based on a macro, from 
// https://users.rust-lang.org/t/how-to-store-async-function-pointer/38343/4
//
// TODO: 
// * See if the whole thing can be done using quote!() without mutating the AST.
// * Write some compile-tests for this macro to make sure that the error messages make sense.
#[proc_macro_attribute]
pub fn goose_client_callback(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut ast: syn::ItemFn = syn::parse(input.clone()).unwrap();    

    let lifetimes: Vec<_> = ast.sig.generics.lifetimes().collect();
    if lifetimes.len() != 1 {
        return quote_spanned!(
            ast.sig.generics.span() => std::compile_error!(
                "please specify the lifetime of your borrowed argument as a generic lifetime")
        ).into()
    }
    let lifetime = lifetimes[0];

    if ast.sig.output != syn::ReturnType::Default {
        return quote_spanned!(
            ast.sig.output.span() => std::compile_error!(
                "return types not supported yet")
        ).into()
    }
    ast.sig.output = syn::parse2(quote! {
        -> ::std::pin::Pin<
            ::std::boxed::Box<
                dyn ::std::future::Future<Output = () > + ::std::marker::Send + #lifetime
            >
        >
    }).unwrap();

    ast.sig.asyncness = None;

    let block = ast.block;
    let block = quote! {
        {
            ::std::boxed::Box::pin(async move { #block })
        }
    };
    ast.block = syn::parse2(block).unwrap();

    proc_macro::TokenStream::from(quote!(#ast))
}
