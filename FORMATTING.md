# Formatting Guidelines

All contributions to Xous must comply with two formatting guidelines:

1. Run `rustfmt` using our `rustfmt.toml` with a nightly toolchain
2. Remove trailing whitespaces

## Resources

Our `rustfmt` requires a Rust nightly toolchain.

Install a nightly toolchain with

`rustup toolchain install nightly`

From here, you may run `rustfmt` on the command line with

`cargo +nightly fmt -p <crate you are editing>`

Unfortunately, one cannot simply run `rustfmt` on a single file with nightly from the command line without changing the default toolchain. However, there are various plugins one can use to help with this.

### vscode

If you are using `vscode`, add these lines to your `settings.json` file:

```json
"rust-analyzer.rustfmt.extraArgs": [
        "+nightly"
    ],
"files.trimTrailingWhitespace": true,
"editor.formatOnSave": true,
```

The default `settings.json` that comes with the repo already has these built in.

### `pre-commit`

[pre-commit](https://pre-commit.com/) is a tool that can be configured to run github actions locally.

The development workflow looks like this:

 1. the developer installs pre-commit on their machine and runs pre-commit install from within the xous-core directory: it'll set up the necessary dependencies, and sets up a local git pre-commit hook to run them
 2. development flows as expected
 3. at commit time, one of two things can happen
    a. code is formatted correctly already, commit gets written
    b. code is not formatted correctly, commit is aborted, code gets formatted automatically

### Other tools

If you have your own workflow, please contribute a hint to this document so that others can benefit from it!
