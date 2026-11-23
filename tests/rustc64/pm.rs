extern crate proc_macro;
use proc_macro::TokenStream;
#[proc_macro]
pub fn answer(t: TokenStream) -> TokenStream { format!("({} + 2)", t).parse().unwrap() }
#[proc_macro_derive(Name)]
pub fn name(t: TokenStream) -> TokenStream {
    let n = t.into_iter().skip_while(|x| x.to_string() != "struct").nth(1).unwrap();
    format!("impl {n} {{ fn name() -> &'static str {{ \"{n}\" }} }}").parse().unwrap()
}
