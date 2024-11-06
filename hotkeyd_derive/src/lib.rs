extern crate proc_macro;
use convert_case::Casing;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};

#[proc_macro_derive(ConfigKebabCase)]
pub fn config_kebab_case(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;

    let variants = match input.data {
        Data::Enum(data_enum) => data_enum.variants,
        _ => panic!("EnumIndex can only be derived for enums"),
    };

    let variant_arms = variants.iter().map(|variant| {
        let variant_name = &variant.ident;
        let kebab_case_name = format!("{}", variant_name).to_case(convert_case::Case::Kebab);
        
        match &variant.fields {
            Fields::Unit => quote! {
                #kebab_case_name => Some(#name::#variant),
            },
            Fields::Unnamed(_) => quote! {
                #kebab_case_name => Some(#name::#variant_name(0)),
            },
            _ => panic!("Only unit and unnamed variants are supported in EnumIndex"),
        }
    });

    let expanded = quote! {
        impl #name {
            pub fn from_config_kebab(config_name: &str) -> Option<#name> {
                match config_name {
                    #(#variant_arms)*
                    _ => None
                }
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_derive(EnumIndex)]
pub fn enum_index(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the enum
    let name = input.ident;

    // Match the input to ensure it's an enum
    let variants = match input.data {
        Data::Enum(data_enum) => data_enum.variants,
        _ => panic!("EnumIndex can only be derived for enums"),
    };

    // Generate discriminant function by assigning indexes to enum variants
    let mut index: u32 = 0;
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
            _ => panic!("Only unit and unnamed variants are supported in EnumIndex"),
        }
    });

    // Generate the final index function
    let expanded = quote! {
        impl #name {
            pub fn index(&self) -> u32 {
                match self {
                    #(#variant_arms),*
                }
            }
        }
    };

    // Convert the generated code into a TokenStream
    TokenStream::from(expanded)
}
