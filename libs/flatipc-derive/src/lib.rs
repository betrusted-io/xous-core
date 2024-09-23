use std::sync::atomic::AtomicUsize;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, parse_macro_input, spanned::Spanned};

fn ast_hash(ast: &syn::DeriveInput) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ast.hash(&mut hasher);
    let full_hash = hasher.finish();

    #[cfg(target_pointer_width = "64")]
    {
        full_hash as usize
    }
    #[cfg(target_pointer_width = "32")]
    {
        (((full_hash >> 32) as u32) ^ (full_hash as u32)) as usize
    }
    #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
    compile_error!("Unsupported target_pointer_width");
}

#[proc_macro_derive(IpcSafe)]
pub fn derive_transmittable(ts: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(ts as syn::DeriveInput);
    derive_transmittable_inner(ast).unwrap_or_else(|e| e).into()
}

fn derive_transmittable_inner(
    ast: DeriveInput,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let ident = ast.ident.clone();
    let transmittable_checks = match &ast.data {
        syn::Data::Struct(r#struct) => generate_transmittable_checks_struct(&ast, r#struct)?,
        syn::Data::Enum(r#enum) => generate_transmittable_checks_enum(&ast, r#enum)?,
        syn::Data::Union(r#union) => generate_transmittable_checks_union(&ast, r#union)?,
    };
    let result = quote! {
        #transmittable_checks

        unsafe impl flatipc::IpcSafe for #ident {}
    };

    Ok(result)
}

#[proc_macro_derive(Ipc)]
pub fn derive_ipc(ts: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(ts as syn::DeriveInput);
    derive_ipc_inner(ast).unwrap_or_else(|e| e).into()
}

fn derive_ipc_inner(ast: DeriveInput) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    // Ensure the thing is using a repr we support.
    ensure_valid_repr(&ast)?;

    let transmittable_checks = match &ast.data {
        syn::Data::Struct(r#struct) => generate_transmittable_checks_struct(&ast, r#struct)?,
        syn::Data::Enum(r#enum) => generate_transmittable_checks_enum(&ast, r#enum)?,
        syn::Data::Union(r#union) => generate_transmittable_checks_union(&ast, r#union)?,
    };

    let ipc_struct = generate_ipc_struct(&ast)?;
    Ok(quote! {
        #transmittable_checks
        #ipc_struct
    })
}

fn ensure_valid_repr(ast: &DeriveInput) -> Result<(), proc_macro2::TokenStream> {
    let mut repr_c = false;
    for attr in ast.attrs.iter() {
        if attr.path().is_ident("repr") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("C") {
                    repr_c = true;
                }
                Ok(())
            })
            .map_err(|e| e.to_compile_error())?;
        }
    }
    if !repr_c {
        Err(syn::Error::new(ast.span(), "Structs must be marked as repr(C) to be IPC-safe")
            .to_compile_error())
    } else {
        Ok(())
    }
}

fn type_to_string(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Array(_type_array) => "Array".to_owned(),
        syn::Type::BareFn(_type_bare_fn) => "BareFn".to_owned(),
        syn::Type::Group(_type_group) => "Group".to_owned(),
        syn::Type::ImplTrait(_type_impl_trait) => "ImplTrait".to_owned(),
        syn::Type::Infer(_type_infer) => "Infer".to_owned(),
        syn::Type::Macro(_type_macro) => "Macro".to_owned(),
        syn::Type::Never(_type_never) => "Never".to_owned(),
        syn::Type::Paren(_type_paren) => "Paren".to_owned(),
        syn::Type::Path(_type_path) => "Path".to_owned(),
        syn::Type::Ptr(_type_ptr) => "Ptr".to_owned(),
        syn::Type::Reference(_type_reference) => "Reference".to_owned(),
        syn::Type::Slice(_type_slice) => "Slice".to_owned(),
        syn::Type::TraitObject(_type_trait_object) => "TraitObject".to_owned(),
        syn::Type::Tuple(_type_tuple) => "Tuple".to_owned(),
        syn::Type::Verbatim(_token_stream) => "Verbatim".to_owned(),
        _ => "Other (Unknown)".to_owned(),
    }
}

fn ensure_type_exists_for(ty: &syn::Type) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    match ty {
        syn::Type::Path(_) => {
            static ATOMIC_INDEX: AtomicUsize = AtomicUsize::new(0);
            let fn_name = format_ident!(
                "assert_type_exists_for_parameter_{}",
                ATOMIC_INDEX.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            );
            Ok(quote! {
                fn #fn_name (_var: #ty) { ensure_is_transmittable::<#ty>(); }
            })
        }
        syn::Type::Tuple(tuple) => {
            let mut check_functions = vec![];
            for ty in tuple.elems.iter() {
                check_functions.push(ensure_type_exists_for(ty)?);
            }
            Ok(quote! {
                #(#check_functions)*
            })
        }
        syn::Type::Array(array) => ensure_type_exists_for(&array.elem),
        _ => Err(syn::Error::new(ty.span(), format!("The type `{}` is unsupported", type_to_string(ty)))
            .to_compile_error()),
    }
}

fn generate_transmittable_checks_enum(
    ast: &syn::DeriveInput,
    enm: &syn::DataEnum,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let mut variants = Vec::new();

    let surrounding_function = format_ident!("ensure_members_are_transmittable_for_{}", ast.ident);
    for variant in &enm.variants {
        let fields = match &variant.fields {
            syn::Fields::Named(fields) => {
                fields.named.iter().map(|f| ensure_type_exists_for(&f.ty)).collect()
            }
            syn::Fields::Unnamed(fields) => {
                fields.unnamed.iter().map(|f| ensure_type_exists_for(&f.ty)).collect()
            }
            syn::Fields::Unit => Vec::new(),
        };

        let mut vetted_fields = vec![];
        for field in fields {
            match field {
                Ok(f) => vetted_fields.push(f),
                Err(e) => return Err(e),
            }
        }

        variants.push(quote! {
                #(#vetted_fields)*
        });
    }

    Ok(quote! {
        #[allow(non_snake_case, dead_code)]
        fn #surrounding_function () {
            pub fn ensure_is_transmittable<T: flatipc::IpcSafe>() {}
            #(#variants)*
        }

    })
}

fn generate_transmittable_checks_struct(
    ast: &syn::DeriveInput,
    strct: &syn::DataStruct,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let surrounding_function = format_ident!("ensure_members_are_transmittable_for_{}", ast.ident);
    let fields = match &strct.fields {
        syn::Fields::Named(fields) => fields.named.iter().map(|f| ensure_type_exists_for(&f.ty)).collect(),
        syn::Fields::Unnamed(fields) => {
            fields.unnamed.iter().map(|f| ensure_type_exists_for(&f.ty)).collect()
        }
        syn::Fields::Unit => Vec::new(),
    };
    let mut vetted_fields = vec![];
    for field in fields {
        match field {
            Ok(f) => vetted_fields.push(f),
            Err(e) => return Err(e),
        }
    }
    Ok(quote! {
        #[allow(non_snake_case, dead_code)]
        fn #surrounding_function () {
            pub fn ensure_is_transmittable<T: flatipc::IpcSafe>() {}
            #(#vetted_fields)*
        }
    })
}

fn generate_transmittable_checks_union(
    ast: &syn::DeriveInput,
    unn: &syn::DataUnion,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let surrounding_function = format_ident!("ensure_members_are_transmittable_for_{}", ast.ident);
    let fields: Vec<Result<proc_macro2::TokenStream, proc_macro2::TokenStream>> =
        unn.fields.named.iter().map(|f| ensure_type_exists_for(&f.ty)).collect();

    let mut vetted_fields = vec![];
    for field in fields {
        match field {
            Ok(f) => vetted_fields.push(f),
            Err(e) => return Err(e),
        }
    }
    Ok(quote! {
        #[allow(non_snake_case, dead_code)]
        fn #surrounding_function () {
            pub fn ensure_is_transmittable<T: flatipc::IpcSafe>() {}
            #(#vetted_fields)*
        }
    })
}

fn generate_ipc_struct(ast: &DeriveInput) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let visibility = ast.vis.clone();
    let ident = ast.ident.clone();
    let ipc_ident = format_ident!("Ipc{}", ast.ident);
    let ident_size = quote! { core::mem::size_of::< #ident >() };
    let padded_size = quote! { (#ident_size + (4096 - 1)) & !(4096 - 1) };
    let padding_size = quote! { #padded_size - #ident_size };
    let hash = ast_hash(ast);

    let build_message = quote! {
        use xous::definitions::{MemoryMessage, MemoryAddress, MemoryRange};
        let mut buf = unsafe { MemoryRange::new(data.as_ptr() as usize, data.len()) }.unwrap();
        let msg = MemoryMessage {
            id: opcode,
            buf,
            offset: MemoryAddress::new(signature),
            valid: None,
        };
    };

    let lend = if cfg!(feature = "xous") {
        quote! {
            #build_message
            xous::send_message(connection, xous::Message::MutableBorrow(msg))?;
        }
    } else {
        quote! {
            flatipc::backend::mock::IPC_MACHINE.lock().unwrap().lend(connection, opcode, signature, 0, &data);
        }
    };

    let try_lend = if cfg!(feature = "xous") {
        quote! {
            #build_message
            xous::try_send_message(connection, xous::Message::MutableBorrow(msg))?;
        }
    } else {
        quote! {
            flatipc::backend::mock::IPC_MACHINE.lock().unwrap().lend(connection, opcode, signature, 0, &data);
        }
    };

    let lend_mut = if cfg!(feature = "xous") {
        quote! {
            #build_message
            xous::send_message(connection, xous::Message::MutableBorrow(msg))?;
        }
    } else {
        quote! {
            flatipc::backend::mock::IPC_MACHINE.lock().unwrap().lend_mut(connection, opcode, signature, 0, &mut data);
        }
    };

    let try_lend_mut = if cfg!(feature = "xous") {
        quote! {
            #build_message
            xous::try_send_message(connection, xous::Message::MutableBorrow(msg))?;
        }
    } else {
        quote! {
            flatipc::backend::mock::IPC_MACHINE.lock().unwrap().lend_mut(connection, opcode, signature, 0, &mut data);
        }
    };

    let memory_messages = if cfg!(feature = "xous") {
        quote! {
            fn from_memory_message<'a>(msg: &'a xous::MemoryMessage) -> Option<&'a Self> {
                if msg.buf.len() < core::mem::size_of::< #ipc_ident >() {
                    return None;
                }
                let signature = msg.offset.map(|offset| offset.get()).unwrap_or_default();
                if signature != #hash {
                    return None;
                }
                unsafe { Some(&*(msg.buf.as_ptr() as *const #ipc_ident)) }
            }

            fn from_memory_message_mut<'a>(msg: &'a mut xous::MemoryMessage) -> Option<&'a mut Self> {
                if msg.buf.len() < core::mem::size_of::< #ipc_ident >() {
                    return None;
                }
                let signature = msg.offset.map(|offset| offset.get()).unwrap_or_default();
                if signature != #hash {
                    return None;
                }
                unsafe { Some(&mut *(msg.buf.as_mut_ptr() as *mut #ipc_ident)) }
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #[repr(C, align(4096))]
        #visibility struct #ipc_ident {
            original: #ident,
            padding: [u8; #padding_size],
        }

        impl core::ops::Deref for #ipc_ident {
            type Target = #ident ;
            fn deref(&self) -> &Self::Target {
                &self.original
            }
        }

        impl core::ops::DerefMut for #ipc_ident {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.original
            }
        }

        impl flatipc::IntoIpc for #ident {
            type IpcType = #ipc_ident;
            fn into_ipc(self) -> Self::IpcType {
                #ipc_ident {
                    original: self,
                    padding: [0; #padding_size],
                }
            }
        }

        unsafe impl flatipc::Ipc for #ipc_ident {
            type Original = #ident ;

            fn from_slice<'a>(data: &'a [u8], signature: usize) -> Option<&'a Self> {
                if data.len() < core::mem::size_of::< #ipc_ident >() {
                    return None;
                }
                if signature != #hash {
                    return None;
                }
                unsafe { Some(&*(data.as_ptr() as *const u8 as *const #ipc_ident)) }
            }

            unsafe fn from_buffer_unchecked<'a>(data: &'a [u8]) -> &'a Self {
                &*(data.as_ptr() as *const u8 as *const #ipc_ident)
            }

            fn from_slice_mut<'a>(data: &'a mut [u8], signature: usize) -> Option<&'a mut Self> {
                if data.len() < core::mem::size_of::< #ipc_ident >() {
                    return None;
                }
                if signature != #hash {
                    return None;
                }
                unsafe { Some(&mut *(data.as_mut_ptr() as *mut u8 as *mut #ipc_ident)) }
            }

            unsafe fn from_buffer_mut_unchecked<'a>(data: &'a mut [u8]) -> &'a mut Self {
                unsafe { &mut *(data.as_mut_ptr() as *mut u8 as *mut #ipc_ident) }
            }

            fn lend(&self, connection: flatipc::CID, opcode: usize) -> Result<(), flatipc::Error> {
                let signature = self.signature();
                let data = unsafe {
                    core::slice::from_raw_parts(
                        self as *const #ipc_ident as *const u8,
                        core::mem::size_of::< #ipc_ident >(),
                    )
                };
                #lend
                Ok(())
            }

            fn try_lend(&self, connection: flatipc::CID, opcode: usize) -> Result<(), flatipc::Error> {
                let signature = self.signature();
                let data = unsafe {
                    core::slice::from_raw_parts(
                        self as *const #ipc_ident as *const u8,
                        core::mem::size_of::< #ipc_ident >(),
                    )
                };
                #try_lend
                Ok(())
            }

            fn lend_mut(&mut self, connection: flatipc::CID, opcode: usize) -> Result<(), flatipc::Error> {
                let signature = self.signature();
                let mut data = unsafe {
                    core::slice::from_raw_parts_mut(
                        self as *mut #ipc_ident as *mut u8,
                        #padded_size,
                    )
                };
                #lend_mut
                Ok(())
            }

            fn try_lend_mut(&mut self, connection: flatipc::CID, opcode: usize) -> Result<(), flatipc::Error> {
                let signature = self.signature();
                let mut data = unsafe {
                    core::slice::from_raw_parts_mut(
                        self as *mut #ipc_ident as *mut u8,
                        #padded_size,
                    )
                };
                #try_lend_mut
                Ok(())
            }

            fn as_original(&self) -> &Self::Original {
                &self.original
            }

            fn as_original_mut(&mut self) -> &mut Self::Original {
                &mut self.original
            }

            fn into_original(self) -> Self::Original {
                self.original
            }

            fn signature(&self) -> usize {
                #hash
            }

            #memory_messages
        }
    })
}
