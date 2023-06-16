;;; SPDX-FileCopyrightText: © 2023 Foundation Devices <hello@foundationdevices.com>
;;; SPDX-License-Identifier: GPL-3.0-or-later
;;;
;;; Commentary:
;;;
;;; Licensed as GPL-3.0-or-later to avoid friction when upstreaming package
;;; definitions to GNU Guix.
;;;
;;; Ideally, this should be a GNU Guix channel and most of the packages that
;;; fit GNU Guix policy should be upstreamed to reduce maintenance work.
;;;
;;; Maybe this could be it's separate channel if the file grows big enough to
;;; justify so.
;;;
;;; Keep package definitions in alphabetic order, inputs of packages as well
;;; too.
;;;
;;; To use this file as basis for development environment:
;;;
;;; guix environment -l guix.scm
;;;
;;; To build and install, run:
;;;
;;; guix package -f guix.scm
;;;
;;; To build packages, but not install:
;;;
;;; guix build -f guix.scm
;;;
;;; For Rust development:
;;;
;;; Due to the way that the cargo-build-system works using this file as a base
;;; for development is not that good for development, this may improve in the
;;; future when other proposed build systems for Rust are implemented in Guix
;;; such as antioxidant which tries to compile crates to libs instead of using
;;; a vendored-dependencies approach that cargo-build-system uses.

(use-modules (gnu packages)
             (gnu packages crates-io)
             (gnu packages libusb)
             (gnu packages python-crypto)
             (gnu packages python-web)
             (gnu packages python-xyz)
             (guix build-system cargo)
             (guix build-system python)
             (guix download)
             (guix gexp)
             ((guix licenses) #:prefix license:)
             (guix packages))

(define-public precursorupdater
  (package
    (name "precursorupdater")
    (version "0.1.3")
    (source (local-file "tools/updater" #:recursive? #t))
    (build-system python-build-system)
    (arguments (list #:tests? #f))
    (propagated-inputs
      (list python-progressbar2
            python-pycryptodome
            python-pyusb
            python-requests))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis "Tool to update Precursor devices operating system")
    (description "This package provides a command line interface application
that can be used to update Precursor devices operating system through the
@acronym{USB, Universal Serial Bus} interface.")
    (license license:asl2.0)))

(define-public rust-aho-corasick-1
  (package
    (inherit rust-aho-corasick-0.7)
    (version "1.0.2")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "aho-corasick" version))
              (file-name (string-append (package-name rust-aho-corasick-0.7)
                                        "-" version ".tar.gz"))
              (modules '((guix build utils)))
              (snippet
                ;; Relax doc-comment version.
                #~(substitute* "Cargo.toml"
                    (("version = \"0\\.3\\.3\"")
                     "version = \"0.3\"")))
              (sha256
                (base32
                  "0has59a3571irggpk5z8c0lpnx8kdx12qf4g2x0560i2y8dwpxj3"))))
    (arguments
     `(#:cargo-inputs
       (("rust-log" ,rust-log-0.4)
        ("rust-memchr" ,rust-memchr-2))
       #:cargo-development-inputs
       (("rust-doc-comment" ,rust-doc-comment-0.3))))))

(define-public rust-darling-0.20
  (package
    (inherit rust-darling-0.14)
    (version "0.20.1")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "darling" version))
              (file-name (string-append (package-name rust-darling-0.14) "-"
                                        version ".tar.gz"))
              (modules '((guix build utils)))
              (snippet
                ;; Relax syn version.
                #~(substitute* "Cargo.toml"
                    (("version = \"2\\.0\\.15\"")
                      "version = \"2\"")))
              (sha256
                (base32
                  "0i1r9d78cysq7231lxa7fil2dc9hkzq7cgwr3qjd0gj6gcmd4n05"))))
    (arguments
     `(#:cargo-inputs
       (("rust-darling-core" ,rust-darling-core-0.20)
        ("rust-darling-macro" ,rust-darling-macro-0.20))
        #:cargo-development-inputs
        (("rust-proc-macro2" ,rust-proc-macro2-1)
         ("rust-quote" ,rust-quote-1)
         ("rust-rustversion" ,rust-rustversion-1)
         ("rust-syn" ,rust-syn-2)
         ("rust-trybuild" ,rust-trybuild-1))))))

(define-public rust-darling-core-0.20
  (package
    (inherit rust-darling-core-0.14)
    (version "0.20.1")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "darling_core" version))
              (file-name (string-append (package-name rust-darling-core-0.14)
                                        "-" version ".tar.gz"))
              (modules '((guix build utils)))
              (snippet
                ;; Relax syn version.
                #~(substitute* "Cargo.toml"
                    (("version = \"2\\.0\\.15\"")
                     "version = \"2\"")))
              (sha256
                (base32
                  "1ss6l190g7zidflpzjkjsyh08i7caly4m0lpbv7f33lz4lpgm2xb"))))
    (arguments
     `(#:cargo-inputs
       (("rust-fnv" ,rust-fnv-1)
        ("rust-ident-case" ,rust-ident-case-1)
        ("rust-proc-macro2" ,rust-proc-macro2-1)
        ("rust-quote" ,rust-quote-1)
        ("rust-strsim" ,rust-strsim-0.10)
        ("rust-syn" ,rust-syn-2))))))

(define-public rust-darling-macro-0.20
  (package
    (inherit rust-darling-macro-0.14)
    (version "0.20.1")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "darling_macro" version))
              (file-name (string-append (package-name rust-darling-macro-0.14)
                                        "-" version ".tar.gz"))
              (modules '((guix build utils)))
              (snippet
                ;; Relax syn version.
                #~(substitute* "Cargo.toml"
                    (("version = \"2\\.0\\.15\"")
                      "version = \"2\"")))
              (sha256
                (base32
                  "06k18lagz6r6gyq1gaa6b9lklqh2k5d9pvqzwv1hkv0jkzzmi8r9"))))
    (arguments
     `(#:cargo-inputs
       (("rust-darling-core" ,rust-darling-core-0.20)
        ("rust-quote" ,rust-quote-1)
        ("rust-syn" ,rust-syn-2))))))

(define-public rust-quick-xml-0.28
  (package
    (inherit rust-quick-xml-0.22)
    (version "0.28.1")
    (source
      (origin
        (method url-fetch)
        (uri (crate-uri "quick-xml" version))
        (file-name (string-append (package-name rust-quick-xml-0.22) "-"
                                  version ".tar.gz"))
        (sha256
          (base32 "1qnpfsn7wvvk44c53cds28541zjk44kd8j7v1daisay43dxskhg5"))))
    (arguments
     `(#:cargo-inputs
       (("rust-arbitrary" ,rust-arbitrary-1)
        ("rust-encoding-rs" ,rust-encoding-rs-0.8)
        ("rust-memchr" ,rust-memchr-2)
        ("rust-serde" ,rust-serde-1)
        ("rust-tokio" ,rust-tokio-1))
       #:cargo-development-inputs
       (("rust-criterion" ,rust-criterion-0.4)
        ("rust-pretty-assertions" ,rust-pretty-assertions-1)
        ("rust-regex" ,rust-regex-1)
        ("rust-serde" ,rust-serde-1)
        ("rust-serde-derive" ,rust-serde-derive-1)
        ("rust-serde-value" ,rust-serde-value-0.7)
        ("rust-tokio-test" ,rust-tokio-test-0.4))))))

(define-public rust-ron-0.8
  (package
    (inherit rust-ron-0.7)
    (version "0.8.0")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "ron" version))
              (file-name (string-append (package-name rust-ron-0.7)
                                        "-" version ".tar.gz"))
              (sha256
                (base32
                  "1zvb2gxn4vv24swwp8a1l9fg5p960w9f9zd9ny05rd8w7c2m22ih"))))
    (arguments
     `(#:tests? #f ;; Tests fail.
       #:cargo-inputs
       (("rust-base64" ,rust-base64-0.13)
        ("rust-bitflags" ,rust-bitflags-1)
        ("rust-indexmap" ,rust-indexmap-1))
       #:cargo-development-inputs
        (("rust-option-set" ,rust-option-set-0.1)
         ("rust-serde-bytes" ,rust-serde-bytes-0.11)
         ("rust-serde-json" ,rust-serde-json-1))))))

;;; 0.8.11 is required by serde-rmp, a lower version can't be used.
(define-public rust-rmp-0.8.11
  (package
    (inherit rust-rmp-0.8)
    (version "0.8.11")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "rmp" version))
              (file-name (string-append (package-name rust-rmp-0.8) "-"
                                        version ".tar.gz"))
              (sha256
                (base32
                  "17rw803xv84csxgd654g7q64kqf9zgkvhsn8as3dbmlg6mr92la4"))))
    (build-system cargo-build-system)
    (arguments
     `(#:cargo-inputs
       (("rust-byteorder" ,rust-byteorder-1)
        ("rust-num-traits" ,rust-num-traits-0.2)
        ("rust-paste" ,rust-paste-1))))))

(define-public rust-rmp-serde-1
  (package
    (inherit rust-rmp-serde-0.15)
    (version "1.1.1")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "rmp-serde" version))
              (file-name (string-append (package-name rust-rmp-serde-0.15) "-"
                                        version ".tar.gz"))
              (sha256
                (base32
                  "0glisa0pcj56dhsaqp5vkqkcqqnb2dcal8kjzf50n8p0jbhkpcf5"))))
    (arguments
     `(#:tests? #f ;; error[E0433]: failed to resolve: use of undeclared crate
                   ;; or module `rmpv`
       #:cargo-inputs
       (("rust-byteorder" ,rust-byteorder-1)
        ("rust-rmp" ,rust-rmp-0.8.11)
        ("rust-serde" ,rust-serde-1))
        #:cargo-development-inputs
        (("rust-serde-bytes" ,rust-serde-bytes-0.11)
         ("rust-serde-derive" ,rust-serde-derive-1))))))

(define-public rust-serde-with-3
  (package
    (name "rust-serde-with")
    (version "3.0.0")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "serde_with" version))
              (file-name (string-append name "-" version ".tar.gz"))
              (modules '((guix build utils)))
              (snippet
                #~(substitute* "Cargo.toml"
                    ;; Relax doc-comment version.
                    (("version = \"0\\.3\\.3\"")
                     "version = \"0.3\"")
                    ;; Relax expect-test version.
                    (("version = \"1\\.3\\.0\"")
                     "version = \"1\"")
                    ;; Relax regex version.
                    (("version = \"1\\.8\\.1\"")
                     "version = \"1\"")))
              (sha256
                (base32
                  "04w5v0siychbb7l3anx57crvv9m3w866ckwjhkq5nf1wdsmdh0lz"))))
    (build-system cargo-build-system)
    (arguments
     `(#:tests? #f ;; Various errors.
       #:cargo-inputs
       (("rust-base64" ,rust-base64-0.21)
        ("rust-chrono" ,rust-chrono-0.4)
        ("rust-doc-comment" ,rust-doc-comment-0.3)
        ("rust-hex" ,rust-hex-0.4)
        ("rust-indexmap" ,rust-indexmap-1)
        ("rust-serde" ,rust-serde-1)
        ("rust-serde-json" ,rust-serde-json-1)
        ("rust-serde-with-macros" ,rust-serde-with-macros-3)
        ("rust-time" ,rust-time-0.3))
       #:cargo-development-inputs
       (("rust-expect-test" ,rust-expect-test-1)
        ("rust-fnv" ,rust-fnv-1)
        ("rust-glob" ,rust-glob-0.3)
        ("rust-mime" ,rust-mime-0.3)
        ("rust-pretty-assertions" ,rust-pretty-assertions-1)
        ("rust-regex" ,rust-regex-1)
        ("rust-rmp-serde" ,rust-rmp-serde-1)
        ("rust-ron" ,rust-ron-0.8)
        ("rust-rustversion" ,rust-rustversion-1)
        ("rust-serde-xml-rs" ,rust-serde-xml-rs-0.6)
        ("rust-serde-json" ,rust-serde-json-1)
        ("rust-serde-test" ,rust-serde-test-1)
        ("rust-serde-yaml" ,rust-serde-yaml-0.9)
        ("rust-version-sync" ,rust-version-sync-0.9))))
    (home-page "https://github.com/jonasbb/serde_with")
    (synopsis "Custom de/serialization functions for Rust's serde")
    (description "Custom de/serialization functions for Rust's serde")
    (license (list license:expat license:asl2.0))))

(define-public rust-serde-with-macros-3
  (package
    (name "rust-serde-with-macros")
    (version "3.0.0")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "serde_with_macros" version))
              (file-name (string-append name "-" version ".tar.gz"))
              (modules '((guix build utils)))
              (snippet
                #~(substitute* "Cargo.toml"
                    ;; Relax expect-test version.
                    (("version = \"1\\.4\\.0\"")
                      "version = \"1\"")
                    ;; Relax trybuild version.
                    (("version = \"1\\.0\\.80\"")
                      "version = \"1\"")))
              (sha256
                (base32
                  "0w5hp31ji9vc5x00qzsn6yxfy16573fn8ppf4bkjrc9gjg9xbizd"))))
    (build-system cargo-build-system)
    (arguments
      `(#:tests? #f  ;; thread 'test_serde_with_dependency' panicked at 'could
                     ;; not read ../serde_with/Cargo.toml: No such file or
                     ;; directory (os error 2)', tests/version_numbers.rs:17:5
        #:cargo-inputs
        (("rust-darling" ,rust-darling-0.20)
         ("rust-proc-macro2" ,rust-proc-macro2-1)
         ("rust-quote" ,rust-quote-1)
         ("rust-syn" ,rust-syn-2))
        #:cargo-development-inputs
        (("rust-expect-test" ,rust-expect-test-1)
         ("rust-pretty-assertions" ,rust-pretty-assertions-1)
         ("rust-rustversion" ,rust-rustversion-1)
         ("rust-serde" ,rust-serde-1)
         ("rust-serde-json" ,rust-serde-json-1)
         ("rust-trybuild" ,rust-trybuild-1)
         ("rust-version-sync" ,rust-version-sync-0.9))))
    (home-page "https://github.com/jonasbb/serde_with/")
    (synopsis "proc-macro library for serde_with")
    (description "proc-macro library for serde_with")
    (license (list license:expat license:asl2.0))))

(define-public rust-serde-xml-rs-0.6
  (package
    (inherit rust-serde-xml-rs-0.5)
    (version "0.6.0")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "serde-xml-rs" version))
              (file-name (string-append (package-name rust-serde-xml-rs-0.5)
                                        "-" version ".tar.gz"))
              (sha256
                (base32
                  "10i7dvd0c1clj4jbljd08qs8466nlymx7ma7k3ncksx1rn7affpv"))))
    (arguments
     `(#:cargo-inputs
       (("rust-log" ,rust-log-0.4)
        ("rust-serde" ,rust-serde-1)
        ("rust-thiserror" ,rust-thiserror-1)
        ("rust-xml-rs" ,rust-xml-rs-0.8))
        #:cargo-development-inputs
        (("rust-docmatic" ,rust-docmatic-0.1)
         ("rust-rstest" ,rust-rstest-0.12)
         ("rust-serde" ,rust-serde-1)
         ("rust-simple-logger" ,rust-simple-logger-2))))))

(define-public rust-serde-yaml-0.9
  (package
    (inherit rust-serde-yaml-0.8)
    (name "rust-serde-yaml")
    (version "0.9.21")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "serde_yaml" version))
              (file-name (string-append (package-name rust-serde-yaml-0.8) "-"
                                        version ".tar.gz"))
              (sha256
                (base32
                  "1714w6f5b2g4svha9r96cirz05mc0d9xfaxkcrabzqvxxkiq9mnr"))))
    (arguments
     `(#:cargo-inputs
       (("rust-indexmap" ,rust-indexmap-1)
        ("rust-itoa" ,rust-itoa-1)
        ("rust-ryu" ,rust-ryu-1)
        ("rust-serde" ,rust-serde-1)
        ("rust-unsafe-libyaml" ,rust-unsafe-libyaml-0.2))
        #:cargo-development-inputs
        (("rust-anyhow" ,rust-anyhow-1)
         ("rust-indoc" ,rust-indoc-2)
         ("rust-serde-derive" ,rust-serde-derive-1))))))

(define-public rust-svd2repl-0.1
  (package
    (name "rust-svd2repl")
    (version "0.1.0")
    (source (local-file "svd2repl" #:recursive? #t))
    (build-system cargo-build-system)
    (arguments
      `(#:cargo-inputs
         (("rust-quick-xml" ,rust-quick-xml-0.28))))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis "Generate Renode platform files from SVD files")
    (description "The svd2repl is a tool to generate Renode platform files
from @acronym{CMSIS-SVD, CMSIS System View Description} files.")
    (license (list license:expat license:asl2.0))))

(define-public rust-svd2utra-0.1
  (package
    (name "rust-svd2utra")
    (version "0.1.17")
    (source (local-file "svd2utra" #:recursive? #t))
    (build-system cargo-build-system)
    (arguments
     `(#:cargo-inputs
       (("rust-quick-xml" ,rust-quick-xml-0.28))))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis "Generate Rust register-access code from SVD files")
    (description "The svd2utra is a tool and a library to generate Rust code
from @acronym{CMSIS-SVD, CMSIS System View Description} files to access register
using the @acronym{UTRA, Unambiguous Thin Register Abstraction} abstractions.")
    (license (list license:expat license:asl2.0))))

(define-public rust-unsafe-libyaml-0.2
  (package
    (name "rust-unsafe-libyaml")
    (version "0.2.8")
    (source (origin
              (method url-fetch)
              (uri (crate-uri "unsafe-libyaml" version))
              (file-name (string-append name "-" version ".tar.gz"))
              (sha256
                (base32
                  "19l0v20x83dvxbr68rqvs9hvawaqd929hia1nldfahlhamm80r8q"))))
    (build-system cargo-build-system)
    (arguments
     `(#:tests? #f ;; error[E0433]: failed to resolve: use of undeclared crate
                   ;; or module `unsafe_libyaml_test_suite`
       #:cargo-development-inputs
       (("rust-pretty-assertions" ,rust-pretty-assertions-1))))
    (home-page "https://github.com/dtolnay/unsafe-libyaml")
    (synopsis "libyaml transpiled to rust by c2rust")
    (description "libyaml transpiled to rust by c2rust")
    (license license:expat)))

(define-public wycheproof-import
  (package
    (name "wycheproof-import")
    (version "0.1.0")
    (source (local-file "tools/wycheproof-import" #:recursive? #t))
    (build-system cargo-build-system)
    (arguments
     `(#:cargo-inputs
       (("rust-eyre" ,rust-eyre-0.6)
        ("rust-serde" ,rust-serde-1)
        ("rust-serde-json" ,rust-serde-json-1)
        ("rust-serde-with" ,rust-serde-with-3))))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis "Tool to import Diffie-Hellman test vectors for Curve25519")
    (description "This package provides a tool to import Diffie-Hellman test
vectors for Curve25519 from the @url{https://github.com/google/wycheproof,
Wycheproof} project for usage on Xous.")
    ;; NOTE: Assuming Apache-2.0, not indicated on the crate.
    (license license:asl2.0)))

;;; Expand with packages that need to be built.
(list precursorupdater
      rust-svd2repl-0.1
      rust-svd2utra-0.1
      wycheproof-import)
