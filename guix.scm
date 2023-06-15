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

;;; Expand with packages that need to be built.
(list precursorupdater
      rust-svd2repl-0.1
      rust-svd2utra-0.1)
