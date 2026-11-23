use proc_macro::TokenStream;

#[proc_macro]
pub fn twice(t: TokenStream) -> TokenStream {
    format!("(2 * ({t}))").parse().unwrap()
}
