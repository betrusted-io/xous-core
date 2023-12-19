== TLS ==

A tls library focused on explicitly trusting Certificate Authority Certificates (requires `--feature tls`).

Typically, operating systems and browsers embed a list of implicitly trusted Root CA certificates.

The tls handshake involves the host offering the client a Certificate signed for authenticity by a CA, with a chain of signatures back to a Root CA. If the client trusts at least one Certificate in this chain of trust, then the handshake may progress to an encrypted connection.

It does not seem in keeping with the Precursor/Xous idea to ask users to trust any such list of Root CA Certificates.

This tls library enables the user to obtain and save trusted CA certificates to the PDDB as a personalised set of trusted Root CA certificates on which to establish a tls connection. This can be as simple for the user as responding to a modal, when making a first connection to an as yet un-trusted host. Just a couple of clicks. The developer need only call `Tls::check_trust()`.

Note that hosts with self-signed Certificates may be probed and trusted (e.g. [chat.signal.org](https://chat.signal.org))

A small set of shellchat commands are included to enact the base functionality:

- `net tls probe <host>` will initiate a modified tls handshake with `<host>`, obtain the certificate chain offered by `<host>`, and immediately terminate the connection. A call to Tls::check_trust() will present the CA certificate chain in a modal to be individually selected and saved to PDDB if trusted.
- `net tls test <host>` will attempt a normal tls handshake with `<host>` based on the trusted Root CA certificates in the PDDB. If the connection is successful, then a simple `get` is emitted, the response accepted, and the connection closed.
- `net tls mozilla` trusts and saves all Root CA's in the [webpki-roots crate](https://crates.io/crates/webpki-roots) - which contains Mozilla's root certificates. (requires `--feature rootCA`)
- `net list` lists all trusted certificates in the PDDB
- `net deleteall` deletes all trusted certificates in the PDDB

These functions are gated by 2 feature flags:
- `tls` includes [der](https://crates.io/crates/der), [ring](https://crates.io/crates/ring) (local patch), [rustls](https://crates.io/crates/rustls), [webpki](https://crates.io/crates/webpki) & [x509-parser](https://crates.io/crates/x509-parser)
- `rootCA` includes the [webpki-roots crate](https://crates.io/crates/webpki-roots)

In keeping with `rustls` & `webpki`, only the critical components of each x509-Certificate are stored in the PDDB under the `tls.trusted` dictionary - as a `rkyv` archive of a `tls::RustTlsOwnedTrustAuthority` object.

The rustls [dangerous_configuration](https://github.com/betrusted-io/xous-core/pull/394/commits/4ea0c8457de8f855723af76546b6ecb7e54661f7) feature is required to modify the tls handshake during a `net tls probe <host>`. This is because, by default, `rustls` drops the connection (and certificate chain) if there is no match to a trusted Root CA Certificate in the `RootStore`. During a `probe` we need to briefly trust all CA certificates in order to get hold of the CA certificate chain, and inspect it.

The shellchat `net tls` commands are are called from `services/shellchat/src/cmds/net_cmd.rs`, but located in `libs/tls/src/cmd.rs` in order to contain the size of `services/shellchat/src/cmds/net_cmd.rs` and to keep the tls cmds close to the implementation.

Native `pddb` calls are used throuought (`std::fs` free)

Considerations:

- there is an outstanding **issue** where the `checkbox` modal does not display options with multiple lines correctly. This impacts the display of fingerprints for some certificates with longer subjects.
- It may be worth considering rendering public `pddb::KEY_NAME_LEN` to allow developer to avoid sending overly long key's which will be rejected by the pddb.
- probably need a shellchat `net tls delete <cert>`
- some broken code is included for a future fix if required [RustlsOwnedTrustAnchor::public_key()](https://github.com/betrusted-io/xous-core/pull/394/commits/4e0298c17ad2c51aa220a88dc69bf2f56e51076f)
- some hosts (eg bunnyfoo.com) emit an `unexpected eof` error during tls handshake 🤷
