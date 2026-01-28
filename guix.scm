;;; Guix development environment for xous-core
;;;
;;; Usage:
;;;   guix shell --pure --development --file=guix.scm
;;;   cargo xtask dabao helloworld

(use-modules (guix packages)
             (guix build-system trivial)
             (guix gexp)
             ((guix licenses) #:prefix license:)
             (gnu packages base)
             (gnu packages bash)
             (gnu packages version-control)
             (gnu packages commencement)
             (rust-xous))

(package
  (name "xous-dev-shell")
  (version "0.9.16")
  (source #f)
  (build-system trivial-build-system)
  (arguments
   (list
    #:modules '((guix build utils))
    #:builder
    #~(begin
        (use-modules (guix build utils))
        (mkdir-p (assoc-ref %outputs "out")))))
  (native-inputs
   `(("rust-xous" ,rust-xous)
     ("git" ,git)
     ("gcc-toolchain" ,gcc-toolchain)
     ;; Essential utilities for --pure shell
     ("bash" ,bash)
     ("coreutils" ,coreutils)
     ("grep" ,grep)
     ("findutils" ,findutils)
     ("sed" ,sed)
     ("diffutils" ,diffutils)
     ("which" ,which)))
  (home-page "https://github.com/betrusted-io/xous-core")
  (synopsis "Xous development shell")
  (description "Development environment for building Xous firmware.
Enter with: guix shell --pure --development --file=guix.scm
Then run: cargo xtask dabao helloworld")
  (license license:asl2.0))
