# PDDB

Please refer to the Xous Book for the [authoritative documentation on the PDDB](https://betrusted.io/xous-book/ch09-00-pddb-overview.html).

Alternatively, documentation for the PDDB can be found in the headers of the source files:

- [main.rs](src/main.rs) contains a glossary, project overview, threat model, auditor's notes, and goals.
- [basis.rs](src/backend/basis.rs) gives details on how the data is organized in
the Basis memory space, and contains most of the code for manipulating dictionary
entries stored within the basis cache.
- [dictionary.rs](src/backend/dictionary.rs) will contain most of the code for
manipulating the keys stored in a dictionary cache entry.
- [keys.rs](src/backend/keys.rs) documents the key cache format and on-disk metadata and data formats.
- [pagetable.rs](src/backend/pagetable.rs) documents the page table format.
- [hw.rs](src/backend/hw.rs) contains all the glue to the SPINOR layer, TRNG, and system time, as well as low-level
routines for formatting the disk.
- [fastspace.rs](src/backend/fastspace.rs) contains the Fast Free Space optimization, which
is a partial record of known free space in the PDDB. The very nature of plausible deniability
requires this quirky structure, as "free space" is the side channel for leaking information about
the existence, or lack of existence, of certain data.

# Why is your RustDoc so Shitty?

Unfortunately, `rustdoc` [can't document binaries](https://github.com/rust-lang/docs.rs/issues/238),
and so the only documentation you'll see generated are for the "frontend" library
portions, which are the API calls you would use to talk to the PDDB. These are
deliberately uninteresting, as we've gone through great effort to make
the bindings are "what you expect" (principle of least surprise).

However, you're probably looking to fix a PDDB bug, audit
the code, or generally figure out what the hell is going on. This means you want
to look at the PDDB as a "binary" and not a "library" -- something RustDoc can't
help you with. As the issue above notes, the fallback is for you to consult this README
file, and literally, read the source (or rather, the paragraphs of plaintext documentation
we've embedded in the source files, waiting for the day when the rustdoc issue is fixed
and these can be turned into something more user-friendly).
