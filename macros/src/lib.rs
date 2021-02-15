// #![deny(warnings)]

extern crate proc_macro;
extern crate rand;
#[macro_use]
extern crate quote;
extern crate core;
extern crate proc_macro2;
#[macro_use]
extern crate syn;

use proc_macro2::Span;
use rand::Rng;
use rand::SeedableRng;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use syn::{parse, spanned::Spanned, Ident, ItemFn, ReturnType, Type, Visibility};

static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

use proc_macro::TokenStream;

/// Attribute to declare the entry point of the program
///
/// **IMPORTANT**: This attribute must appear exactly *once* in the dependency graph. Also, if you
/// are using Rust 1.30 the attribute must be used on a reachable item (i.e. there must be no
/// private modules between the item and the root of the crate); if the item is in the root of the
/// crate you'll be fine. This reachability restriction doesn't apply to Rust 1.31 and newer releases.
///
/// The specified function will be called by the reset handler *after* RAM has been initialized.
/// If present, the FPU will also be enabled before the function is called.
///
/// The type of the specified function must be `[unsafe] fn() -> !` (never ending function)
///
/// # Properties
///
/// The entry point will be called by the reset handler. The program can't reference to the entry
/// point, much less invoke it.
///
/// # Examples
///
/// - Simple entry point
///
/// ``` no_run
/// # #![no_main]
/// # use riscv_rt_macros::entry;
/// #[entry]
/// fn main() -> ! {
///     loop {
///         /* .. */
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn xous_main(args: TokenStream, input: TokenStream) -> TokenStream {
    let f = parse_macro_input!(input as ItemFn);

    // check the function signature
    let valid_signature = f.sig.constness.is_none()
        && f.sig.asyncness.is_none()
        && f.vis == Visibility::Inherited
        && f.sig.abi.is_none()
        && f.sig.inputs.is_empty()
        && f.sig.generics.params.is_empty()
        && f.sig.generics.where_clause.is_none()
        && f.sig.variadic.is_none()
        && match f.sig.output {
            ReturnType::Default => false,
            ReturnType::Type(_, ref ty) => matches!(**ty, Type::Never(_)),
        };

    if !valid_signature {
        return parse::Error::new(
            f.span(),
            "`#[xous_main]` function must have signature `[unsafe] fn() -> !`",
        )
        .to_compile_error()
        .into();
    }

    if !args.is_empty() {
        return parse::Error::new(Span::call_site(), "This attribute accepts no arguments")
            .to_compile_error()
            .into();
    }

    // XXX should we blacklist other attributes?
    let attrs = f.attrs;
    let unsafety = f.sig.unsafety;
    let hash = random_ident();
    let stmts = f.block.stmts;

    let r = quote!(
        #[export_name = "xous_entry"]
        #(#attrs)*
        pub #unsafety fn #hash() -> ! {
            xous::init();
            #(#stmts)*
        }

        xous::maybe_main!();
    );
    r.into()
}

// Creates a random identifier
/*
   Historical note -- this identifier was inherited from the Cortex libraries.
   Apparently, it serves just to prove that the initializing function was run at
   all, and does not create any sort of security property. It could be replaced
   with a "magic number" instead and achieve the same goal. From reading the rationale
   behind why the Cortex ecosystem does this, it's that a name like "__main" could accidentally
   be used by a developer and cause the system to be unsafe, and so by going with
   a pseudorandom identifier, it discourages programmers from accidental
   copy/pasta of well-known symbols and skipping certain initializations that are
   critical in the runtime to guarantee the safety properties that Rust depends
   upon.

   Safety properties meaning, the Rust memory system has to reason about the type
   safety and initialization state of a program, and this starts with a base set
   of assumptions. The runtime is responsible for setting up these assumptions,
   and if they are wrong then the whole Rust memory system analysis is for naught.
   Because the Rust compiler can't "reason beyond the runtime", it becomes a hazard
   that a programmer unwittingly names their "main" function with a well-known
   symbol that doesn't have the proper initializations, and become confused as to
   why their programs don't work. Thus it seems that this random identifier is
   a way to "prove" that the run-time did its thing, without relying upon a symbol
   or name that could just be copy/pasted by the programmer without also copy/pasting
   its semantic significance.

   This is not a "security-critical" random number because it's more of a check on
   programmer behavior; any attacker with the ability to write or modify this number
   also has the ability to modify the initialization routines anyways, and therefore
   promoting this to a true cryptographic random number doesn't solve any problem
   or necessarily protect the Rust type safety system from attacks against the
   initialization frameworks. To protect against that, the code base needs to be
   hashed and signed and checked against a signature, and not rely upon random
   identifiers which have no cryptographically essential correlation with the code around it.
 */
fn random_ident() -> Ident {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let count: u64 = CALL_COUNT.fetch_add(1, Ordering::SeqCst) as u64;
    let mut seed: [u8; 16] = [0; 16];

    for (i, v) in seed.iter_mut().take(8).enumerate() {
        *v = ((secs >> (i * 8)) & 0xFF) as u8
    }

    for (i, v) in seed.iter_mut().skip(8).enumerate() {
        *v = ((count >> (i * 8)) & 0xFF) as u8
    }

    let mut rng = rand::rngs::SmallRng::from_seed(seed);
    Ident::new(
        &(0..16)
            .map(|i| {
                if i == 0 || rng.gen() {
                    (b'a' + rng.gen::<u8>() % 25) as char
                } else {
                    (b'0' + rng.gen::<u8>() % 10) as char
                }
            })
            .collect::<String>(),
        Span::call_site(),
    )
}
