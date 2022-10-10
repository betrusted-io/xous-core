use proc_macro::{self, TokenStream};
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
                    let typ = field.ty.clone();

                    let set_fn_name = format_ident!("set_{}", ident);
                    let fn_name = format_ident!("{}", ident);

                    let read = quote! {
                        impl Manager {
                            // pub fn #set_fn_name(&mut self, value: #typ) {
                            //     self.prefs.#ident = value;
                            //     self.pddb_store(&self.prefs).unwrap()
                            // }

                            // pub fn #fn_name(&mut self) -> #typ {
                            //     self.prefs = self.pddb_get().unwrap();
                            //     self.prefs.#ident
                            // }

                            pub fn #set_fn_name(&mut self, value: #typ) -> Result<(), Error> {
                                let bytes: Vec<u8> = match bincode::encode_to_vec(value, bincode::config::standard()) {
                                    Ok(ret) => Ok(ret),
                                    Err(err) => return Err(Error::EncodeError(err)),
                                };

                                self.pddb_store_key(#fn_name, &bytes)
                            }

                            pub fn #fn_name(&mut self) -> Result<#typ, Error> {
                                let bytes = self.pddb_get_key("#fn_name")?;
                                let ret: #typ = match bincode::decode_from_slice(&bytes, bincode::config::standard()) {
                                    Ok((data, _)) => data,
                                    Err(err) => return Err(Error::DecodeError(err)),
                                };

                                Ok(ret)
                            }
                        }
                    };
                    println!("code: {}", read);
                    panic!();
                    //rw_methods.extend(read);
                }
            }
            _ => (),
        },
        _ => {
            abort!(ident.span(), "This macro can only be used on structs");
        }
    };

    rw_methods.into()
}
