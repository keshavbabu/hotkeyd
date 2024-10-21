extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

// Procedural macro definition
#[proc_macro_derive(EnumOrdering)]
pub fn enum_ordering(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the enum
    let name = input.ident;

    // Match the input to ensure it's an enum
    let variants = match input.data {
        Data::Enum(data_enum) => data_enum.variants,
        _ => panic!("EnumOrdering can only be derived for enums"),
    };

    // Generate discriminant function by assigning indexes to enum variants
    let mut index = 0;
    let variant_arms = variants.iter().map(|variant| {
        let variant_name = &variant.ident;
        let current_index = index;
        index += 1;
        
        match &variant.fields {
            Fields::Unit => quote! {
                #name::#variant_name => #current_index
            },
            // For `Unknown(u32)` or similar, you can still return a discriminant value
            Fields::Unnamed(_) => quote! {
                #name::#variant_name(_) => #current_index
            },
            _ => panic!("Only unit and unnamed variants are supported in KeyDiscriminant"),
        }
    });

    // Generate the final key_discriminant function
    let expanded = quote! {
        impl #name {
            pub fn index(&self) -> usize {
                match self {
                    #(#variant_arms),*
                }
            }
        }
    };

    // Convert the generated code into a TokenStream
    TokenStream::from(expanded)
}
