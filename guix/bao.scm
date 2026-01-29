;;; GNU Guix package definitions for baochip builds
;;;
;;; This module provides packages for building Xous firmware images
;;; for baochip/dabao hardware targets.
;;;
;;; Approach: Uses cargo's [patch."git-url"] sections to redirect ALL git
;;; dependencies to local vendored directories. This preserves Cargo.lock
;;; compatibility and avoids dependency re-resolution issues.

(define-module (bao)
  #:use-module (guix packages)
  #:use-module (guix build-system gnu)
  #:use-module (guix build-system trivial)
  #:use-module (guix gexp)
  #:use-module (guix utils)
  #:use-module ((guix licenses) #:prefix license:)
  #:use-module (gnu packages base)
  #:use-module (gnu packages compression)
  #:use-module (gnu packages version-control)
  #:use-module (gnu packages commencement)
  #:use-module (srfi srfi-1)
  #:use-module (ice-9 popen)
  #:use-module (ice-9 rdelim)
  #:use-module (rust-xous)
  #:use-module (bao-crates))

;;; Version configuration (mirrors flake.nix structure)
(define %xous-git-tag "0.9.16")           ; git describe --abbrev=0
(define %xous-git-tag-rev-count 7276)     ; git rev-list --count $(git describe --abbrev=0)

;;; Helper to run git command at evaluation time
(define (git-command . args)
  "Run a git command in the repo directory and return output, or #f on failure."
  (let ((dir (dirname (current-source-directory))))
    (false-if-exception
     (let* ((port (apply open-pipe* OPEN_READ "git" "-C" dir args))
            (output (read-line port))
            (status (close-pipe port)))
       (and (zero? (status:exit-val status))
            (string? output)
            output)))))

;;; Git revision (full 40-char hash) - detected at evaluation time
;;; Falls back to zeros if not in git repo or working tree is dirty
(define %git-rev
  (or (and (git-command "diff" "--quiet")      ; clean working tree?
           (git-command "rev-parse" "HEAD"))   ; get commit hash
      "0000000000000000000000000000000000000000"))

;;; Short hash for version string (8 chars like git describe)
(define %git-hash (substring %git-rev 0 8))

;;; Count of commits since tag (0 if not in git repo)
(define %since-tag-rev-count
  (or (false-if-exception
       (let ((total (git-command "rev-list" "--count" "HEAD")))
         (and total (- (string->number total) %xous-git-tag-rev-count))))
      0))

;;; Computed version string: v{tag}-{count-since-tag}-g{hash}
(define %xous-version
  (string-append "v" %xous-git-tag "-"
                 (number->string %since-tag-rev-count) "-g"
                 %git-hash))

;;; Local source from current repository (works for both local dev and CI)
;;; Uses dirname to get parent of guix/ directory (the repo root)
(define xous-core-local-source
  (local-file (dirname (current-source-directory))
              #:recursive? #t
              #:select? (lambda (file stat)
                          (not (or (string-contains file "/target/")
                                   (string-contains file "/.git/"))))))

;;; Git dependency metadata
;;; Each entry: (input-name origin git-url ((crate-name . subdir) ...))
;;; subdir is relative path within checkout, "." for root
(define %git-dependencies
  `(("git-armv7" ,rust-armv7-git
     "https://github.com/Foundation-Devices/armv7.git"
     (("armv7" . ".")))
    ("git-atsama5d27" ,rust-atsama5d27-git
     "https://github.com/Foundation-Devices/atsama5d27.git"
     (("atsama5d27" . ".")
      ("utralib" . "utralib")))
    ("git-com-rs" ,rust-com-rs-git
     "https://github.com/betrusted-io/com_rs"
     (("com_rs" . ".")))
    ("git-curve25519-dalek" ,rust-curve25519-dalek-git
     "https://github.com/betrusted-io/curve25519-dalek.git"
     (("curve25519-dalek" . "curve25519-dalek")
      ("curve25519-dalek-derive" . "curve25519-dalek-derive")))
    ("git-engine-25519" ,rust-engine-25519-git
     "https://github.com/betrusted-io/xous-engine-25519.git"
     (("engine-25519" . ".")))
    ("git-engine25519-as" ,rust-engine25519-as-git
     "https://github.com/betrusted-io/engine25519-as.git"
     (("engine25519-as" . ".")))
    ("git-ring-xous" ,rust-ring-xous-git
     "https://github.com/betrusted-io/ring-xous"
     (("ring" . ".")))
    ("git-rqrr" ,rust-rqrr-git
     "https://github.com/betrusted-io/rqrr.git"
     (("rqrr" . ".")))
    ("git-sha2-xous" ,rust-sha2-xous-git
     "https://github.com/betrusted-io/hashes.git"
     (("sha2" . "sha2")))
    ("git-simple-fatfs" ,rust-simple-fatfs-git
     "https://github.com/betrusted-io/simple-fatfs.git"
     (("simple-fatfs" . ".")))
    ("git-usb-device" ,rust-usb-device-git
     "https://github.com/betrusted-io/usb-device.git"
     (("usb-device" . ".")))
    ("git-usbd-serial" ,rust-usbd-serial-git
     "https://github.com/betrusted-io/usbd-serial.git"
     (("usbd-serial" . ".")))
    ("git-xous-usb-hid" ,rust-xous-usb-hid-git
     "https://github.com/betrusted-io/xous-usb-hid.git"
     (("xous-usb-hid" . ".")))))

;;; Derive git URL to local path mappings from %git-dependencies
;;; Returns list of (crate-name git-url input-name subdir)
(define (git-deps->mappings deps)
  (append-map
   (lambda (dep)
     (let ((input-name (car dep))
           (git-url (caddr dep))
           (crate-mappings (cadddr dep)))
       (map (lambda (mapping)
              (list (car mapping) git-url input-name (cdr mapping)))
            crate-mappings)))
   deps))

(define %git-mappings (git-deps->mappings %git-dependencies))

;;; Helper to create baochip build packages with vendored dependencies
(define* (make-bao-build name xtask-cmd
                          #:key
                          (target-dir "riscv32imac-unknown-none-elf")
                          (crate-inputs '()))
  (package
    (name name)
    (version %xous-git-tag)
    (source xous-core-local-source)
    (build-system gnu-build-system)
    (arguments
     (list
      #:phases
      #~(modify-phases %standard-phases
            (delete 'configure)
            (delete 'check)

            ;; Phase 1: Patch versioning to avoid git calls
            (add-after 'unpack 'patch-versioning
              (lambda _
                (when (file-exists? "tools/src/sign_image.rs")
                  (substitute* "tools/src/sign_image.rs"
                    (("SemVer::from_git\\(\\)\\?\\.into\\(\\)")
                     (string-append "\"" #$%xous-version "\".parse::<SemVer>().unwrap().into()"))))
                (substitute* "xtask/src/versioning.rs"
                  (("let gitver = output\\.stdout;")
                   "let gitver = std::env::var(\"XOUS_VERSION\").map(|s| s.into_bytes()).unwrap_or(output.stdout);"))
                (when (file-exists? "tools/src/swap_writer.rs")
                  (substitute* "tools/src/swap_writer.rs"
                    (("Command::new\\(\"git\"\\)\\.args\\(&\\[\"rev-parse\", \"HEAD\"\\]\\)\\.output\\(\\)\\.expect\\(\"Failed to execute command\"\\)")
                     (string-append
                      "std::env::var(\"GIT_REV\").map(|s| std::process::Output { "
                      "status: std::process::ExitStatus::default(), "
                      "stdout: s.into_bytes(), stderr: vec![] }).unwrap_or_else(|_| "
                      "Command::new(\"git\").args(&[\"rev-parse\", \"HEAD\"]).output()"
                      ".expect(\"Failed to execute command\"))"))))))

            ;; Phase 2: Set up crates.io vendor directory
            (add-after 'patch-versioning 'setup-vendor
              (lambda* (#:key inputs #:allow-other-keys)
                (use-modules (ice-9 popen)
                             (ice-9 rdelim))
                (let ((vendor-dir (string-append (getcwd) "/vendor")))
                  (mkdir-p vendor-dir)
                  (for-each
                   (lambda (input)
                     (let* ((name (car input))
                            (path (cdr input)))
                       (when (string-prefix? "crate-" name)
                         (let* ((file-name (basename path))
                                (crate-name (substring file-name 5 (- (string-length file-name) 7)))
                                (crate-dir (string-append vendor-dir "/" crate-name))
                                (port (open-input-pipe (string-append "sha256sum " path)))
                                (checksum-line (read-line port))
                                (_ (close-pipe port))
                                (checksum (car (string-split checksum-line #\space))))
                           (mkdir-p crate-dir)
                           (invoke "tar" "xzf" path "-C" crate-dir "--strip-components=1")
                           (call-with-output-file (string-append crate-dir "/.cargo-checksum.json")
                             (lambda (port)
                               (format port "{\"files\":{},\"package\":\"~a\"}" checksum)))))))
                   inputs))))

            ;; Phase 3: Set up git dependencies
            ;; Note: Package-specific patches (atsama5d27, xous-usb-hid, rqrr)
            ;; are handled by origin snippets in bao-crates.scm
            (add-after 'setup-vendor 'setup-git-deps
              (lambda* (#:key inputs #:allow-other-keys)
                (use-modules (ice-9 textual-ports)
                             (ice-9 regex))
                (let ((git-vendor-dir (string-append (getcwd) "/git-vendor")))
                  (mkdir-p git-vendor-dir)

                  ;; Copy git checkouts and fix permissions
                  (for-each
                   (lambda (input)
                     (let* ((name (car input))
                            (path (cdr input)))
                       (when (string-prefix? "git-" name)
                         (let ((dest-dir (string-append git-vendor-dir "/" name)))
                           (copy-recursively path dest-dir)
                           (for-each (lambda (f) (chmod f #o755))
                                     (find-files dest-dir ".*" #:directories? #t))))))
                   inputs)

                  ;; Clean up Cargo.toml files in git checkouts (single pass):
                  ;; - Remove [workspace] sections (causes resolution issues)
                  ;; - Remove [dev-dependencies] (causes issues with rand, rusb, etc.)
                  (for-each
                   (lambda (cargo-toml)
                     (let ((content (call-with-input-file cargo-toml get-string-all)))
                       (when (or (string-contains content "[workspace]")
                                 (string-contains content "[dev-dependencies]"))
                         (call-with-output-file cargo-toml
                           (lambda (port)
                             ;; First remove [workspace] sections via regex
                             (let* ((modified content)
                                    (modified (regexp-substitute/global
                                               #f "\\[workspace\\]\n?" modified 'pre 'post))
                                    (modified (regexp-substitute/global
                                               #f "members *= *\\[([^]]|\n)*\\]\n?" modified 'pre 'post))
                                    (modified (regexp-substitute/global
                                               #f "exclude *= *\\[([^]]|\n)*\\]\n?" modified 'pre 'post)))
                               ;; Then remove [dev-dependencies] line-by-line
                               ;; (avoids corrupting feature arrays containing brackets)
                               (let* ((lines (string-split modified #\newline))
                                      (in-dev-deps #f)
                                      (filtered
                                       (filter
                                        (lambda (line)
                                          (cond
                                           ((string-prefix? "[dev-dependencies]" (string-trim line))
                                            (set! in-dev-deps #t) #f)
                                           ((and in-dev-deps (string-prefix? "[" (string-trim line)))
                                            (set! in-dev-deps #f) #t)
                                           (in-dev-deps #f)
                                           (else #t)))
                                        lines)))
                                 (display (string-join filtered "\n") port))))))))
                   (find-files git-vendor-dir "^Cargo\\.toml$")))))

            ;; Phase 4: Patch ALL Cargo.toml files to convert git deps to path deps
            ;; This must happen BEFORE cargo runs, as cargo tries to fetch git sources
            ;; Mappings derived from %git-dependencies: (crate-name git-url input-name subdir)
            (add-after 'setup-git-deps 'patch-cargo-toml-git-deps
              (lambda _
                (use-modules (ice-9 textual-ports)
                             (ice-9 regex))
                (let ((git-vendor-dir (string-append (getcwd) "/git-vendor"))
                      (git-mappings '#$%git-mappings))
                  (for-each
                   (lambda (cargo-toml)
                     (let ((content (call-with-input-file cargo-toml get-string-all)))
                       ;; Only process files that contain git dependencies
                       (when (string-contains content "git = \"https://github.com")
                         (call-with-output-file cargo-toml
                           (lambda (port)
                             (let ((modified content))
                               ;; Replace git = "URL" with path = "LOCAL"
                               (for-each
                                (lambda (mapping)
                                  (let* ((crate-name (car mapping))
                                         (git-url (cadr mapping))
                                         (local-dir (caddr mapping))
                                         (subdir (cadddr mapping))
                                         (local-path (if (string=? subdir ".")
                                                         (string-append git-vendor-dir "/" local-dir)
                                                         (string-append git-vendor-dir "/" local-dir "/" subdir)))
                                         (git-pattern (string-append "git *= *\"" (regexp-quote git-url) "\""))
                                         (path-replacement (string-append "path = \"" local-path "\"")))
                                    (set! modified
                                          (regexp-substitute/global #f git-pattern modified
                                                                    'pre path-replacement 'post))))
                                git-mappings)
                               ;; Remove branch/rev attributes
                               (set! modified
                                     (regexp-substitute/global #f ", *branch *= *\"[^\"]+\"" modified 'pre 'post))
                               (set! modified
                                     (regexp-substitute/global #f ", *rev *= *\"[^\"]+\"" modified 'pre 'post))
                               (set! modified
                                     (regexp-substitute/global #f "\n *branch *= *\"[^\"]+\"[^\n]*" modified 'pre 'post))
                               (set! modified
                                     (regexp-substitute/global #f "\n *rev *= *\"[^\"]+\"[^\n]*" modified 'pre 'post))
                               (display modified port)))))))
                   (find-files "." "^Cargo\\.toml$")))))

            ;; Phase 5: Set up cargo config
            (add-after 'patch-cargo-toml-git-deps 'setup-cargo
              (lambda* (#:key inputs #:allow-other-keys)
                (let* ((rust-xous (assoc-ref inputs "rust-xous"))
                       (vendor-dir (string-append (getcwd) "/vendor"))
                       ;; Offline vendor config (shared between main and locales)
                       (vendor-config
                        (string-append
                         "[source.crates-io]\n"
                         "replace-with = \"vendored-sources\"\n\n"
                         "[source.vendored-sources]\n"
                         "directory = \"" vendor-dir "\"\n\n"
                         "[net]\n"
                         "offline = true\n")))
                  ;; Set up environment
                  (setenv "HOME" (getcwd))
                  (setenv "CARGO_HOME" (string-append (getcwd) "/.cargo"))
                  (mkdir-p (getenv "CARGO_HOME"))
                  (setenv "PATH" (string-append rust-xous "/bin:" (getenv "PATH")))

                  ;; Main cargo config with build settings
                  (call-with-output-file ".cargo/config.toml"
                    (lambda (port)
                      (display
                       (string-append
                        "[alias]\n"
                        "xtask = \"run --package xtask --\"\n\n"
                        "[build]\n"
                        "rustflags = [\"--cfg\", \"crossbeam_no_atomic_64\"]\n\n"
                        "[target.riscv32imac-unknown-xous-elf]\n"
                        "rustflags = [\"--cfg\", 'curve25519_dalek_backend=\"u32e_backend\"']\n\n"
                        "[target.riscv32imac-unknown-none-elf]\n"
                        "rustflags = [\"--cfg\", 'curve25519_dalek_backend=\"fiat\"']\n\n"
                        vendor-config)
                       port)))

                  ;; Locales has its own Cargo.lock, needs vendor config only
                  (mkdir-p "locales/.cargo")
                  (call-with-output-file "locales/.cargo/config.toml"
                    (lambda (port) (display vendor-config port))))))

            ;; Phase 6: Build
            (replace 'build
              (lambda* (#:key inputs #:allow-other-keys)
                (setenv "XOUS_VERSION" #$%xous-version)
                (setenv "GIT_REV" #$%git-rev)
                (setenv "CARGO_INCREMENTAL" "0")
                (setenv "RUSTFLAGS" (string-append "-C codegen-units=1 --remap-path-prefix="
                                                   (getcwd) "=/build"))
                (setenv "SOURCE_DATE_EPOCH" "1")
                ;; Run xtask build with --no-verify (we're using local patches)
                (invoke "cargo" "xtask" #$@(string-split xtask-cmd #\space) "--no-verify")))

            ;; Phase 7: Install
            (replace 'install
              (lambda* (#:key outputs #:allow-other-keys)
                (let* ((out (assoc-ref outputs "out"))
                       (target-path (string-append "target/" #$target-dir "/release")))
                  (mkdir-p out)
                  (for-each
                   (lambda (pattern)
                     (for-each
                      (lambda (file)
                        (copy-file file (string-append out "/" (basename file))))
                      (find-files target-path pattern)))
                   '("\\.uf2$" "\\.img$" "\\.bin$"))))))))
    (native-inputs
     `(("rust-xous" ,rust-xous)
       ("git" ,git)
       ("tar" ,tar)
       ("gzip" ,gzip)
       ("coreutils" ,coreutils)
       ;; Add all crates as inputs
       ,@(map (lambda (crate)
                `(,(string-append "crate-" (origin-file-name crate)) ,crate))
              crate-inputs)
       ;; Add git dependency inputs
       ,@(map (lambda (dep)
                `(,(car dep) ,(cadr dep)))
              %git-dependencies)))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis (string-append "Xous " name " firmware"))
    (description (string-append "Xous firmware build for " name " target. "
                                "Built with xtask command: " xtask-cmd))
    (license license:asl2.0)))

;;; Individual build targets

(define-public bao1x-boot0
  (make-bao-build "bao1x-boot0" "bao1x-boot0"
                   #:target-dir "riscv32imac-unknown-none-elf"
                   #:crate-inputs (lookup-cargo-inputs 'bao1x-boot0)))

(define-public bao1x-boot1
  (make-bao-build "bao1x-boot1" "bao1x-boot1"
                   #:target-dir "riscv32imac-unknown-none-elf"
                   #:crate-inputs (lookup-cargo-inputs 'bao1x-boot1)))

(define-public bao1x-alt-boot1
  (make-bao-build "bao1x-alt-boot1" "bao1x-alt-boot1"
                   #:target-dir "riscv32imac-unknown-none-elf"
                   #:crate-inputs (lookup-cargo-inputs 'bao1x-alt-boot1)))

(define-public bao1x-baremetal-dabao
  (make-bao-build "bao1x-baremetal-dabao" "bao1x-baremetal-dabao"
                   #:target-dir "riscv32imac-unknown-none-elf"
                   #:crate-inputs (lookup-cargo-inputs 'bao1x-baremetal-dabao)))

(define-public dabao
  (make-bao-build "dabao" "dabao"
                   #:target-dir "riscv32imac-unknown-xous-elf"
                   #:crate-inputs (lookup-cargo-inputs 'dabao)))

(define-public dabao-helloworld
  (make-bao-build "dabao-helloworld" "dabao helloworld"
                   #:target-dir "riscv32imac-unknown-xous-elf"
                   #:crate-inputs (lookup-cargo-inputs 'dabao-helloworld)))

(define-public baosec
  (make-bao-build "baosec" "baosec"
                   #:target-dir "riscv32imac-unknown-xous-elf"
                   #:crate-inputs (lookup-cargo-inputs 'baosec)))


;;; Combined bootloader package
(define-public bootloader
  (package
    (name "bootloader")
    (version %xous-git-tag)
    (source #f)
    (build-system trivial-build-system)
    (arguments
     (list
      #:modules '((guix build utils))
      #:builder
      #~(begin
          (use-modules (guix build utils))
          (let ((out (assoc-ref %outputs "out"))
                (boot0 #$(this-package-input "boot0"))
                (boot1 #$(this-package-input "boot1"))
                (alt-boot1 #$(this-package-input "alt-boot1")))
            (mkdir-p out)
            (for-each
             (lambda (src)
               (for-each
                (lambda (file)
                  (copy-file file (string-append out "/" (basename file))))
                (find-files src "\\.(uf2|img|bin)$")))
             (list boot0 boot1 alt-boot1))))))
    (inputs
     `(("boot0" ,bao1x-boot0)
       ("boot1" ,bao1x-boot1)
       ("alt-boot1" ,bao1x-alt-boot1)))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis "Combined Xous bootloader package")
    (description "Combined bootloader package containing boot0, boot1, and alt-boot1
for bao1x hardware.")
    (license license:asl2.0)))

;;; Development shell - use with: guix shell -L . -D xous-dev-shell
(define-public xous-dev-shell
  (package
    (name "xous-dev-shell")
    (version %xous-git-tag)
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
       ("coreutils" ,coreutils)
       ("grep" ,grep)
       ("findutils" ,findutils)
       ("sed" ,sed)
       ("diffutils" ,diffutils)
       ("which" ,which)))
    (home-page "https://github.com/betrusted-io/xous-core")
    (synopsis "Xous development shell")
    (description "Development environment for building Xous firmware.
Enter with: guix shell -L . -D xous-dev-shell
Then run: cargo xtask <command>")
    (license license:asl2.0)))

;; Default export
dabao-helloworld
