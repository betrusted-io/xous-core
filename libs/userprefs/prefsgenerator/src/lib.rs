use proc_macro::{self, TokenStream};
use proc_macro2::Ident;
use proc_macro_error::{abort, proc_macro_error};
use quote::{format_ident, quote};
use syn::{parse_macro_input, DeriveInput, FieldsNamed};

#[proc_macro_error]
#[proc_macro_derive(GetterSetter)]
pub fn getter_setter(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input);

    let mut rw_methods = quote! {};

    match data {
        syn::Data::Struct(s) => match s.fields {
            syn::Fields::Named(FieldsNamed { named, .. }) => {
                for field in &named {
                    let ident = field.ident.as_ref().unwrap().clone();
                    let ident_str = ident.to_string();
                    let typ = field.ty.clone();

                    let set_fn_name = format_ident!("set_{}", ident);
                    let fn_name = format_ident!("{}", ident);
                    let fn_name_or_default = format_ident!("{}_or_default", ident);
                    let fn_name_or_value = format_ident!("{}_or_value", ident);

                    let read = quote! {
                        impl Manager {
                            pub fn #set_fn_name(&self, value: #typ) -> Result<(), Error> {
                                let bytes: Vec<u8> = match bincode::encode_to_vec(value, bincode::config::standard()) {
                                    Ok(ret) => ret,
                                    Err(err) => return Err(Error::EncodeError(err)),
                                };

                                self.pddb_store_key(#ident_str, &bytes)
                            }

                            pub fn #fn_name(&self) -> Result<#typ, Error> {
                                let bytes = self.pddb_get_key(#ident_str)?;
                                let ret: #typ = match bincode::decode_from_slice(&bytes, bincode::config::standard()) {
                                    Ok((data, _)) => data,
                                    Err(err) => return Err(Error::DecodeError(err)),
                                };

                                Ok(ret)
                            }

                            pub fn #fn_name_or_default(&self) -> Result<#typ, Error> {
                                let bytes = self.pddb_get_key(#ident_str)?;
                                let ret: #typ = match bincode::decode_from_slice(&bytes, bincode::config::standard()) {
                                    Ok((data, _)) => data,
                                    Err(_) => #typ::default(),
                                };

                                Ok(ret)
                            }

                            pub fn #fn_name_or_value(&self, value: #typ) -> Result<#typ, Error> {
                                let bytes = self.pddb_get_key(#ident_str)?;
                                let ret: #typ = match bincode::decode_from_slice(&bytes, bincode::config::standard()) {
                                    Ok((data, _)) => data,
                                    Err(_) => value,
                                };

                                Ok(ret)
                            }
                        }
                    };

                    rw_methods.extend(read);
                }

                let ident_map: Vec<Ident> =
                    named.iter().map(|field| field.ident.as_ref().unwrap().clone()).collect();

                let ident_fn_calls: Vec<Ident> = named
                    .iter()
                    .map(|field| format_ident!("{}_or_default", field.ident.as_ref().unwrap().clone()))
                    .collect();

                // create the "all" method
                let all = quote! {
                    impl Manager {
                        pub fn all(&self) -> Result<UserPrefs, Error> {

                            Ok(UserPrefs {
                                #( #ident_map:self.#ident_fn_calls()? ),*
                            })
                        }
                    }
                };

                rw_methods.extend(all);
            }
            _ => (),
        },
        _ => {
            abort!(ident.span(), "This macro can only be used on structs");
        }
    };

    rw_methods.into()
}
