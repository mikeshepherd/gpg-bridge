use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    DeriveInput, Ident, Type, parse_macro_input,
    punctuated::Punctuated,
    token::{Comma, Semi},
    visit::Visit,
};

struct Step {
    ident: Ident,
    ty: Type,
}

struct TypeVisitor<'a> {
    tokens: &'a mut TokenStream,
}

impl<'a> TypeVisitor<'a> {
    fn new(tokens: &'a mut TokenStream) -> TypeVisitor<'a> {
        TypeVisitor { tokens }
    }
}

impl<'ast> Visit<'ast> for TypeVisitor<'ast> {
    fn visit_generic_argument(&mut self, i: &'ast syn::GenericArgument) {
        quote! { ::< }.to_tokens(self.tokens);
        syn::visit::visit_generic_argument(self, i);
        quote! { > }.to_tokens(self.tokens);
    }
    fn visit_ident(&mut self, i: &'ast proc_macro2::Ident) {
        i.to_tokens(self.tokens);
    }
}

impl ToTokens for Step {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Step { ident, ty } = self;
        let mut toks = TokenStream::new();
        let mut visitor = TypeVisitor::new(&mut toks);
        visitor.visit_type(ty);
        let expanded = quote! { let #ident = #toks::decode(reader)? };
        expanded.to_tokens(tokens)
    }
}

struct Parsed {
    name: Ident,
    names: Punctuated<Ident, Comma>,
    steps: Punctuated<Step, Semi>,
}

fn parse_data(input: &DeriveInput) -> Result<Parsed, syn::Error> {
    // Used in the quasi-quotation below as `#name`.
    let name = &input.ident;
    let struct_data = match &input.data {
        syn::Data::Struct(v) => v,
        _ => {
            return Err(syn::Error::new_spanned(name, "Must be struct type"));
        }
    };
    let mut names: Punctuated<Ident, Comma> = Punctuated::new();
    let mut steps: Punctuated<Step, Semi> = Punctuated::new();
    struct_data.clone().fields.into_iter().for_each(|f| {
        let ident = f.ident.unwrap();
        let ty = f.ty;
        names.push(ident.clone());
        steps.push(Step { ident, ty })
    });
    Ok(Parsed {
        name: name.to_owned(),
        names,
        steps,
    })
}

#[proc_macro_derive(Decoder)]
pub fn derive_decoder(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse the input tokens into a syntax tree.
    let input = parse_macro_input!(input as DeriveInput);
    match parse_data(&input) {
        Ok(parsed) => generate_decode(parsed),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_decode(parsed: Parsed) -> proc_macro::TokenStream {
    let Parsed { name, names, steps } = parsed;
    let expanded = quote! {
      #[automatically_derived]
      impl Decode for #name {
        type Item = #name;
        fn decode_with_size(reader: &mut BytesMut) -> crate::protocol::ProtocolResult<(usize, Self::Item)> {
          #steps;
          Ok((0, #name {
            #names
          }))
        }
      }
    };

    expanded.into()
}

#[proc_macro_derive(BlobDecoder)]
pub fn derive_blob_decoder(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse the input tokens into a syntax tree.
    let input = parse_macro_input!(input as DeriveInput);
    match parse_data(&input) {
        Ok(parsed) => generate_blob_decode(parsed),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_blob_decode(parsed: Parsed) -> proc_macro::TokenStream {
    let Parsed { name, names, steps } = parsed;
    let expanded = quote! {
      #[automatically_derived]
      impl DecodeBlob for #name {
        type Item = #name;
        fn decode_blob(reader: &mut BytesMut) -> crate::protocol::ProtocolResult<Self::Item> {
          #steps;
          Ok(#name {
            #names
          })
        }
      }
    };
    expanded.into()
}
