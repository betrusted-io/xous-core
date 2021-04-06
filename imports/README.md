# Imports

## Build Not Working: Why You're Most Likely Looking at This

If your build isn't working due to a failed import reference, it's probably because
one of the `xous-core` developers accidentally checked in a reference to a local directory
in one or more of the `Cargo.toml` files within this subdirectory.

Most likely, you can find the offending `Cargo.toml` file, and remove the local directory
reference and replace it with the commented-out gitref. In the worst case, the developer
failed to push up changes in the dependant crate, at which point you should open an issue
to get that fixed.

## #TIL: patch.crates-io
When we did this, we didn't know there was a thing such as `[patch.crates-io]` that allows for
overrides in cargo paths. Probably this system should be refactored to use this,
instead of the methodology outlined here.

## Rationale
We depend upon some packages that aren't in `crates.io` yet, but are also not
integrated into the `xous-core` repo. Reasons for this range from the preference
of the package's originator, to local cross-project dependencies (such as the `com-rs`
package, which is the API between the `betrusted-ec` firmware and `xous-core`) that
don't make sense to publish as a crate.

These are instead hosted in a separate, standa-lone github repo. The methodology
in this case is to refer to the git commit ref in the `Cargo.toml` for `xous-core`
integration.

However, we have situations where multiple crates within `xous-core` need
to refer to the same git commit ref, which leads to the opportunity for
inconsistent versions between sub-crates within `xous-core`.

`imports` are simple packages that re-export the contents of a stand-alone package,
allowing us a single location, common to all sub-crates within `xous-core`, to define a
single git commit ref. For local development, the git commit ref is replaced
with a local directory path.

The convention is to name the imported package based upon its root name
and then add `-ref` to the end, and then to `use import-ref as import` to
resolve the import into the correct name space.
