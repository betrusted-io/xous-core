;;; Xous-core crate sources
;;;
;;; This file supplements Guix's (gnu packages rust-crates) with
;;; crates that are missing or have different versions needed for xous-core.
;;;
;;; Git dependencies: manually defined from Cargo.lock [source] sections
;;; Crates.io deps: generated with guix import crate

(define-module (xous-crates)
  #:use-module (guix packages)
  #:use-module (guix download)
  #:use-module (guix git-download)
  #:use-module (guix build-system cargo)
  #:export (;; Helper function
            crate-source
            ;; Git dependency origins
            rust-armv7-git
            rust-atsama5d27-git
            rust-com-rs-git
            rust-curve25519-dalek-git
            rust-engine-25519-git
            rust-engine25519-as-git
            rust-ring-xous-git
            rust-rqrr-git
            rust-sha2-xous-git
            rust-simple-fatfs-git
            rust-usb-device-git
            rust-usbd-serial-git
            rust-xous-usb-hid-git
            ;; Crate inputs lists
            bao1x-boot0-crate-inputs
            ;; Lookup function
            lookup-cargo-inputs))

;;;
;;; Helper function to create crate source origins
;;;

(define (crate-source name version hash)
  (origin
    (method url-fetch)
    (uri (crate-uri name version))
    (file-name (string-append "rust-" name "-" version ".tar.gz"))
    (sha256 (base32 hash))))

;;;
;;; Git dependencies (not on crates.io)
;;; To compute hashes: git clone <url> && cd <dir> && git checkout <commit> && guix hash -rx .
;;;

;; armv7 0.2.1 from Foundation-Devices
(define rust-armv7-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/Foundation-Devices/armv7.git")
          (commit "e37a72f420a5e6633d9f7802c7fd094ccf8ca1f9")))
    (file-name "rust-armv7-0.2.1-checkout")
    (sha256 (base32 "0q720v21rcpq0pi1di0yjdbqzmbgk7xhayangwb6fhyn9bbql48l"))))

;; atsama5d27 0.1.0 + utralib 0.1.18 from Foundation-Devices (same repo)
(define rust-atsama5d27-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/Foundation-Devices/atsama5d27.git")
          (commit "9e83a502e68384754bb328a5717c56f34c8618f7")))
    (file-name "rust-atsama5d27-0.1.0-checkout")
    (sha256 (base32 "0gjfcrn8dia0nqag035c431qaxw7qa13h8mb17kwgbfkabg2w6b9"))))

;; com_rs 0.1.0 from betrusted-io
(define rust-com-rs-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/com_rs")
          (commit "891bdd3ca8e41f81510d112483e178aea3e3a921")))
    (file-name "rust-com-rs-0.1.0-checkout")
    (sha256 (base32 "1c2yjy3chygfrjzdynm0160xmfw0cviyrfyskh78xqy0bvf43di8"))))

;; curve25519-dalek 4.1.2 + curve25519-dalek-derive 0.1.1 from betrusted-io
(define rust-curve25519-dalek-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/curve25519-dalek.git")
          (commit "6f0ac06fe077d0bb465b30743a0c0da09decaa24")))
    (file-name "rust-curve25519-dalek-4.1.2-checkout")
    (sha256 (base32 "0m68xpz6gkcxja75nq9i3v8kh9l1bvnzkm882hx46i6angrnkbay"))))

;; engine-25519 0.1.0 from betrusted-io
(define rust-engine-25519-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/xous-engine-25519.git")
          (commit "63d3d1f30736022e791deaacf4dd62c00b42fe2e")))
    (file-name "rust-engine-25519-0.1.0-checkout")
    (sha256 (base32 "15792rdkn2lzg35cj9c122bkhbwm248p9mqrv34811y8fhxf4v6l"))))

;; engine25519-as 0.1.0 from betrusted-io
(define rust-engine25519-as-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/engine25519-as.git")
          (commit "775e8406eb4aad08f05ae10619fcb4ca891ba0a6")))
    (file-name "rust-engine25519-as-0.1.0-checkout")
    (sha256 (base32 "0mmfv30kahpa402gbsgr4lgp1lkqnszz5bvcc0881h21ddymdxrz"))))

;; ring 0.17.7 from betrusted-io (ring-xous fork)
(define rust-ring-xous-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/ring-xous")
          (commit "5f86cb10bebd521a45fb3abb06995200aeda2948")))
    (file-name "rust-ring-0.17.7-checkout")
    (sha256 (base32 "1yvzghjwvyp2l4kf6j5m3spwz2nsci64gj4nsx06mgcgpm616389"))))

;; rqrr 0.10.0 from betrusted-io
(define rust-rqrr-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/rqrr.git")
          (commit "388fc6c0b7ee6cd7e5a2261a9d67a0c0692184f1")))
    (file-name "rust-rqrr-0.10.0-checkout")
    (sha256 (base32 "0il0cr1wpkj32f1bih4xq2bbf2iwanmlavlq9f6ls0c80qn6vz1d"))))

;; sha2 0.10.8 from betrusted-io (hashes fork)
;; Commit from Cargo.lock: ab2ab59c41f294eef1d90ac768a5d94a08c12d63
(define rust-sha2-xous-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/hashes.git")
          (commit "ab2ab59c41f294eef1d90ac768a5d94a08c12d63")))
    (file-name "rust-sha2-0.10.8-checkout")
    (sha256 (base32 "0p1g7nzw0k5pcxc74pkw9grgk4ic29bq3gk1khgm8lyhjdryyyh6"))))

;; simple-fatfs 0.1.0-alpha.1 from betrusted-io
(define rust-simple-fatfs-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/simple-fatfs.git")
          (commit "83b886a5a849ad03d07350cb9f9af6b32da53e7c")))
    (file-name "rust-simple-fatfs-0.1.0-checkout")
    (sha256 (base32 "1xfbn8nwlblcgv1vqps7khcm8c8agfidvimvk1kchig74vn33axh"))))

;; usb-device 0.2.8 from betrusted-io
(define rust-usb-device-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/usb-device.git")
          (commit "7ab6ba8120f3d7b6e0904a7ef33e27e192617fe9")))
    (file-name "rust-usb-device-0.2.8-checkout")
    (sha256 (base32 "1q52rhkaacly5jq1zbxigij083bnjjbvd3qgkjhmcx3p7i0zzhrk"))))

;; usbd-serial 0.1.1 from betrusted-io
(define rust-usbd-serial-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/usbd-serial.git")
          (commit "c20edaf89d74466e06773f66110a38156c6eb4a7")))
    (file-name "rust-usbd-serial-0.1.1-checkout")
    (sha256 (base32 "1vj5s488n9gpxlkb99l3jvyj8b5z2qp4m9dh4kj258k7bkk3x7m3"))))

;; xous-usb-hid 0.4.3 from betrusted-io
(define rust-xous-usb-hid-git
  (origin
    (method git-fetch)
    (uri (git-reference
          (url "https://github.com/betrusted-io/xous-usb-hid.git")
          (commit "793ec00243c525f2d17dc1d3a185abdeec57aaf6")))
    (file-name "rust-xous-usb-hid-0.4.3-checkout")
    (sha256 (base32 "172bw2p6hj3478ns5c06nlwy1hqflrhz3ngnmriy7zxp4bpqsr5y"))))

;;;
;;; Crates.io dependencies for locales (older versions from locales/Cargo.lock)
;;;

(define rust-aho-corasick-0.7.18
  (crate-source "aho-corasick" "0.7.18"
                "0vv50b3nvkhyy7x7ip19qnsq11bqlnffkmj2yx2xlyk5wzawydqy"))

(define rust-memchr-2.4.1
  (crate-source "memchr" "2.4.1"
                "0smq8xzd40njqpfzv5mghigj91fzlfrfg842iz8x0wqvw2dw731h"))

(define rust-proc-macro2-1.0.36
  (crate-source "proc-macro2" "1.0.36"
                "0adh6gvs31x6pfwmygypmzrv1jc7kjq568vsqcfaxk7vhdc2sd67"))

(define rust-quote-1.0.15
  (crate-source "quote" "1.0.15"
                "0id1q0875pvhkg0mlb5z8gzdm2g2rbbz76bfzhv331lrm2b3wkc6"))

(define rust-regex-1.6.0
  (crate-source "regex" "1.6.0"
                "12wqvyh4i75j7pc8sgvmqh4yy3qaj4inc4alyv1cdf3lf4kb6kjc"))

(define rust-regex-syntax-0.6.27
  (crate-source "regex-syntax" "0.6.27"
                "0i32nnvyzzkvz1rqp2qyfxrp2170859z8ck37jd63c8irrrppy53"))

(define rust-ryu-1.0.9
  (crate-source "ryu" "1.0.9"
                "17qlxkqm4h8h9xqj6rh2vnmwxyzikbsj5w223chmr5l2qx8bgd3k"))

(define rust-serde-1.0.135
  (crate-source "serde" "1.0.135"
                "0axdrddc78biwmv7pn0r7z2q2iw1c5a6d55prpfs4kj96daj7y9c"))

(define rust-serde-json-1.0.78
  (crate-source "serde_json" "1.0.78"
                "11c0fm7wb2wydlxmq9ziqfjwxl9j1cl0jxq16az49z8fryj1ng6j"))

(define rust-unicode-xid-0.2.2
  (crate-source "unicode-xid" "0.2.2"
                "1wrkgcw557v311dkdb6n2hrix9dm2qdsb1zpw7pn79l03zb85jwc"))

;;;
;;; Crates.io dependencies
;;; For bao1x-boot0 and other xous-core packages
;;;

(define rust-arbitrary-int-1.3.0
  (crate-source "arbitrary-int" "1.3.0"
                "19g0raiw8wr6n9iyxcslc0cqlxrz12ihqxqjp5bpadkpim9rfll2"))

(define rust-autocfg-1.1.0
  (crate-source "autocfg" "1.1.0"
                "1ylp3cb47ylzabimazvbz9ms6ap784zhb6syaz6c1jqpmcmq0s6l"))

(define rust-autocfg-0.1.8
  (crate-source "autocfg" "0.1.8"
                "0y4vw4l4izdxq1v0rrhvmlbqvalrqrmk60v1z0dqlgnlbzkl7phd"))

(define rust-bare-metal-0.2.4
  (crate-source "bare-metal" "0.2.4"
                "0nkkbajx2hydm97lbnia73rww83hb5s0d3b3h0z4ab9vv69z7jm3"))

(define rust-bitbybit-1.4.0
  (crate-source "bitbybit" "1.4.0"
                "0qi7rvg5sckf56i5hklmhkcma9zbgkhgkykm04khkqh7mf4pl67c"))

(define rust-bitfield-0.13.2
  (crate-source "bitfield" "0.13.2"
                "06g7jb5r2b856vnhx76081fg90jvmy61kjqcfjysgmd5hclvvbs6"))

(define rust-bit-field-0.9.0
  (crate-source "bit_field" "0.9.0"
                "0mjxkfcz2biq469iwsrj7jrj1cr54qrpssxbfiwn22chky86b1zd"))

(define rust-bitflags-1.3.2
  (crate-source "bitflags" "1.3.2"
                "12ki6w8gn1ldq7yz9y680llwk5gmrhrzszaa17g1sbrw2r2qvwxy"))

(define rust-block-buffer-0.10.4
  (crate-source "block-buffer" "0.10.4"
                "0w9sa2ypmrsqqvc20nhwr75wbb5cjr4kkyhpjm1z1lv2kdicfy1h"))

(define rust-bytemuck-derive-1.10.2
  (crate-source "bytemuck_derive" "1.10.2"
                "1zvmjmw1sdmx9znzm4dpbb2yvz9vyim8w6gp4z256l46qqdvvazr"))

(define rust-bytemuck-1.24.0
  (crate-source "bytemuck" "1.24.0"
                "1x65wc9kwf0dfnmglkl8r46d29pfl7yilll5wh9bcf0g6a0gbg8z"))

(define rust-cfg-if-1.0.0
  (crate-source "cfg-if" "1.0.0"
                "1za0vb97n4brpzpv8lsbnzmq5r8f2b0cpqqr0sy8h5bn751xxwds"))

(define rust-cpufeatures-0.2.17
  (crate-source "cpufeatures" "0.2.17"
                "10023dnnaghhdl70xcds12fsx2b966sxbxjq5sxs49mvxqw5ivar"))

(define rust-critical-section-1.2.0
  (crate-source "critical-section" "1.2.0"
                "02ylhcykxjc40xrfhk1lwc21jqgz4dbwv3jr49ymw733c51yl3kr"))

(define rust-compiler-builtins-0.1.108
  (crate-source "compiler_builtins" "0.1.108"
                "1s976arcq20152cg6zpr442in886pi3v2yv8q8cxf73i559wb2yn"))

(define rust-crypto-common-0.1.6
  (crate-source "crypto-common" "0.1.6"
                "1cvby95a6xg7kxdz5ln3rl9xh66nz66w46mm3g56ri1z5x815yqv"))

(define rust-curve25519-dalek-derive-0.1.1
  (crate-source "curve25519-dalek-derive" "0.1.1"
                "1cry71xxrr0mcy5my3fb502cwfxy6822k4pm19cwrilrg7hq4s7l"))

(define rust-digest-0.10.7
  (crate-source "digest" "0.10.7"
                "14p2n6ih29x81akj097lvz7wi9b6b9hvls0lwrv7b6xwyy0s5ncy"))

(define rust-ed25519-2.2.3
  (crate-source "ed25519" "2.2.3"
                "0lydzdf26zbn82g7xfczcac9d7mzm3qgx934ijjrd5hjpjx32m8i"))

(define rust-either-1.9.0
  (crate-source "either" "1.9.0"
                "01qy3anr7jal5lpc20791vxrw0nl6vksb5j7x56q2fycgcyy8sm2"))

(define rust-embedded-hal-1.0.0
  (crate-source "embedded-hal" "1.0.0"
                "128bb4h3kw8gvz6w7xa0z0j6nrk5jhm3aa7v350clkh0nzz906in"))

(define rust-enum-dispatch-0.3.12
  (crate-source "enum_dispatch" "0.3.12"
                "03l998igqfzkykmj8i5qlbwhv2id9jn98fkkl82lv3dvg0q32cwg"))

(define rust-fiat-crypto-0.3.0
  (crate-source "fiat-crypto" "0.3.0"
                "094z20x40qws7ca8khvjqssiajf5sy1b1cgdwqd0cl6kvlr1xkb4"))

(define rust-generic-array-0.14.7
  (crate-source "generic-array" "0.14.7"
                "16lyyrzrljfq424c3n8kfwkqihlimmsg5nhshbbp48np3yjrqr45"))

(define rust-hex-literal-0.4.1
  (crate-source "hex-literal" "0.4.1"
                "0iny5inkixsdr41pm2vkqh3fl66752z5j5c0cdxw16yl9ryjdqkg"))

(define rust-lazy-static-1.4.0
  (crate-source "lazy_static" "1.4.0"
                "0in6ikhw8mgl33wjv6q6xfrb5b9jr16q8ygjy803fay4zcisvaz2"))

(define rust-linked-list-allocator-0.10.5
  (crate-source "linked_list_allocator" "0.10.5"
                "11k2dv6v5kq45kbvahll434f9iwfw0vsyaycp76q3vh5ahzldyls"))

(define rust-lock-api-0.4.11
  (crate-source "lock_api" "0.4.11"
                "0iggx0h4jx63xm35861106af3jkxq06fpqhpkhgw0axi2n38y5iw"))

(define rust-log-0.4.22
  (crate-source "log" "0.4.22"
                "093vs0wkm1rgyykk7fjbqp2lwizbixac1w52gv109p5r4jh0p9x7"))

(define rust-memchr-2.7.4
  (crate-source "memchr" "2.7.4"
                "18z32bhxrax0fnjikv475z7ii718hq457qwmaryixfxsl2qrmjkq"))

(define rust-num-derive-0.4.2
  (crate-source "num-derive" "0.4.2"
                "00p2am9ma8jgd2v6xpsz621wc7wbn1yqi71g15gc3h67m7qmafgd"))

(define rust-num-enum-derive-0.5.11
  (crate-source "num_enum_derive" "0.5.11"
                "16f7r4jila0ckcgdnfgqyhhb90w9m2pdbwayyqmwcci0j6ygkgyw"))

(define rust-num-enum-0.5.11
  (crate-source "num_enum" "0.5.11"
                "1japmqhcxwn1d3k7q8jw58y7xfby51s16nzd6dkj483cj2pnqr0z"))

(define rust-num-traits-0.2.18
  (crate-source "num-traits" "0.2.18"
                "0yjib8p2p9kzmaz48xwhs69w5dh1wipph9jgnillzd2x33jz03fs"))

(define rust-once-cell-1.19.0
  (crate-source "once_cell" "1.19.0"
                "14kvw7px5z96dk4dwdm1r9cqhhy2cyj1l5n5b29mynbb8yr15nrz"))

(define rust-paste-1.0.15
  (crate-source "paste" "1.0.15"
                "02pxffpdqkapy292harq6asfjvadgp1s005fip9ljfsn9fvxgh2p"))

(define rust-ppv-lite86-0.2.17
  (crate-source "ppv-lite86" "0.2.17"
                "1pp6g52aw970adv3x2310n7glqnji96z0a9wiamzw89ibf0ayh2v"))

(define rust-proc-macro2-1.0.86
  (crate-source "proc-macro2" "1.0.86"
                "0xrv22p8lqlfdf1w0pj4si8n2ws4aw0kilmziwf0vpv5ys6rwway"))

(define rust-quick-xml-0.28.2
  (crate-source "quick-xml" "0.28.2"
                "1lfr3512x0s0i9kbyglyzn0rq0i1bvd2mqqfi8gs685808rfgr8c"))

(define rust-quote-1.0.35
  (crate-source "quote" "1.0.35"
                "1vv8r2ncaz4pqdr78x7f138ka595sp2ncr1sa2plm4zxbsmwj7i9"))

(define rust-rand-chacha-0.3.1
  (crate-source "rand_chacha" "0.3.1"
                "123x2adin558xbhvqb8w4f6syjsdkmqff8cxwhmjacpsl1ihmhg6"))

(define rust-rand-core-0.6.4
  (crate-source "rand_core" "0.6.4"
                "0b4j2v4cb5krak1pv6kakv4sz6xcwbrmy2zckc32hsigbrwy82zc"))

(define rust-riscv-macros-0.2.0
  (crate-source "riscv-macros" "0.2.0"
                "1g6v1w3gp2fdliw7fj2spb1r7cpri60jxq8vls1wqvdgl4gami78"))

(define rust-riscv-pac-0.2.0
  (crate-source "riscv-pac" "0.2.0"
                "0dnlqpv126jg8nak1h3xp3l235ph2f1n812szf6cdh6c769r1241"))

(define rust-riscv-0.14.0
  (crate-source "riscv" "0.14.0"
                "0g1ndd9yal2x6mr2d9ydvs6fbj6cx6f45wha03z4k881kb3p25hg"))

(define rust-rustc-version-0.2.3
  (crate-source "rustc_version" "0.2.3"
                "02h3x57lcr8l2pm0a645s9whdh33pn5cnrwvn5cb57vcrc53x3hk"))

(define rust-rustc-version-0.4.0
  (crate-source "rustc_version" "0.4.0"
                "0rpk9rcdk405xhbmgclsh4pai0svn49x35aggl4nhbkd4a2zb85z"))

(define rust-rustc-std-workspace-core-1.0.0
  (crate-source "rustc-std-workspace-core" "1.0.0"
                "1309xhwyai9xpz128xrfjqkmnkvgjwddznmj7brbd8i8f58zamhr"))

(define rust-scopeguard-1.2.0
  (crate-source "scopeguard" "1.2.0"
                "0jcz9sd47zlsgcnm1hdw0664krxwb5gczlif4qngj2aif8vky54l"))

(define rust-semver-parser-0.7.0
  (crate-source "semver-parser" "0.7.0"
                "18vhypw6zgccnrlm5ps1pwa0khz7ry927iznpr88b87cagr1v2iq"))

(define rust-semver-0.9.0
  (crate-source "semver" "0.9.0"
                "00q4lkcj0rrgbhviv9sd4p6qmdsipkwkbra7rh11jrhq5kpvjzhx"))

(define rust-semver-1.0.21
  (crate-source "semver" "1.0.21"
                "1c49snqlfcx93xym1cgwx8zcspmyyxm37xa2fyfgjx1vhalxfzmr"))

(define rust-signature-2.2.0
  (crate-source "signature" "2.2.0"
                "1pi9hd5vqfr3q3k49k37z06p7gs5si0in32qia4mmr1dancr6m3p"))

(define rust-spinning-top-0.2.5
  (crate-source "spinning_top" "0.2.5"
                "1c6x734rlvvhjw1prk8k3y7d5z65459br6pzl2ila564yjib37jv"))

(define rust-subtle-2.6.1
  (crate-source "subtle" "2.6.1"
                "14ijxaymghbl1p0wql9cib5zlwiina7kall6w7g89csprkgbvhhk"))

(define rust-syn-1.0.109
  (crate-source "syn" "1.0.109"
                "0ds2if4600bd59wsv7jjgfkayfzy3hnazs394kz6zdkmna8l3dkj"))

(define rust-syn-2.0.87
  (crate-source "syn" "2.0.87"
                "0bd3mfcswvn4jkrp7ich5kk58kmpph8412yxd36nsfnh8vilrai5"))

;; rkyv and its dependencies (for xous-ipc)
(define rust-bytes-1.5.0
  (crate-source "bytes" "1.5.0"
                "08w2i8ac912l8vlvkv3q51cd4gr09pwlg3sjsjffcizlrb0i5gd2"))

(define rust-equivalent-1.0.1
  (crate-source "equivalent" "1.0.1"
                "1malmx5f4lkfvqasz319lq6gb3ddg19yzf9s8cykfsgzdmyq0hsl"))

(define rust-hashbrown-0.14.3
  (crate-source "hashbrown" "0.14.3"
                "012nywlg0lj9kwanh69my5x67vjlfmzfi9a0rq4qvis2j8fil3r9"))

(define rust-indexmap-2.2.2
  (crate-source "indexmap" "2.2.2"
                "087mafd9f98rp1xk2jc1rsp5yyqz63yi30cy8yx6c8s14bj2ljw2"))

(define rust-munge-0.4.1
  (crate-source "munge" "0.4.1"
                "1pqrlhq0l29mcmqd86xill3465yj1bc9pzq6pw5gdbabr0w2s534"))

(define rust-munge-macro-0.4.1
  (crate-source "munge_macro" "0.4.1"
                "0pifls5cmx8561wh4hv2way838grybga1v5yrk8gf4sg33cc3d8v"))

(define rust-ptr-meta-0.3.0
  (crate-source "ptr_meta" "0.3.0"
                "147a6z4qz35gipj9k0d2yh4wygmibhaqsna59vs0d5izdpv7d7py"))

(define rust-ptr-meta-derive-0.3.0
  (crate-source "ptr_meta_derive" "0.3.0"
                "1l9jznaz85cchixyp07v6sxcvjadsyq6lmhjbh98sk0v2pdlwhfa"))

(define rust-rancor-0.1.0
  (crate-source "rancor" "0.1.0"
                "0iyr19x1aryadcyc2zwjbwmskkkjqfbvrjp4l37d3f9434bggxfa"))

(define rust-rend-0.5.1
  (crate-source "rend" "0.5.1"
                "1xqykrcqn2xxqi3ixns9jm9z2989prb3ca60hp4i5nz4b4ciy753"))

(define rust-rkyv-0.8.8
  (crate-source "rkyv" "0.8.8"
                "0ar177cszl8x4ralyx41w9s3vq7mckk65vimc3m1k62ndh3jfl1r"))

(define rust-rkyv-derive-0.8.8
  (crate-source "rkyv_derive" "0.8.8"
                "0bwvsi4kwizy75s7n2qvd63rg5rfa8pw6lh88rzg04289fvq5jq9"))

(define rust-tinyvec-1.6.0
  (crate-source "tinyvec" "1.6.0"
                "0l6bl2h62a5m44jdnpn7lmj14rd44via8180i7121fvm73mmrk47"))

(define rust-tinyvec-macros-0.1.1
  (crate-source "tinyvec_macros" "0.1.1"
                "081gag86208sc3y6sdkshgw3vysm5d34p431dzw0bshz66ncng0z"))

(define rust-uuid-1.10.0
  (crate-source "uuid" "1.10.0"
                "0503gvp08dh5mnm3f0ffqgisj6x3mbs53dmnn1lm19pga43a1pw1"))

(define rust-typenum-1.17.0
  (crate-source "typenum" "1.17.0"
                "09dqxv69m9lj9zvv6xw5vxaqx15ps0vxyy5myg33i0kbqvq0pzs2"))

(define rust-unicode-ident-1.0.12
  (crate-source "unicode-ident" "1.0.12"
                "0jzf1znfpb2gx8nr8mvmyqs1crnv79l57nxnbiszc7xf7ynbjm1k"))

(define rust-version-check-0.9.4
  (crate-source "version_check" "0.9.4"
                "0gs8grwdlgh0xq660d7wr80x14vxbizmd8dbp29p2pdncx8lp1s9"))

(define rust-xous-riscv-0.5.6
  (crate-source "xous-riscv" "0.5.6"
                "06j61dgylcf3ahs5lq0kgmlpx3g2jfxcihjw6yy49yb9asym3v33"))

(define rust-xous-0.9.69
  (crate-source "xous" "0.9.69"
                "1qm5ic0p8mbb00zqz8v84h9lpg1m557fyvxhs63al6fixmngzbmc"))

(define rust-zeroize-1.8.1
  (crate-source "zeroize" "1.8.1"
                "1pjdrmjwmszpxfd7r860jx54cyk94qk59x13sc307cvr5256glyf"))


;; Additional crates (auto-generated from Cargo.lock)
(define rust-adler-1.0.2
  (crate-source "adler" "1.0.2"
                "1zim79cvzd5yrkzl3nyfx0avijwgk9fqv3yrscdy1cc79ih02qpj"))

(define rust-aead-0.5.2
  (crate-source "aead" "0.5.2"
                "1c32aviraqag7926xcb9sybdm36v5vh9gnxpn4pxdwjc50zl28ni"))

(define rust-aes-gcm-siv-0.11.1
  (crate-source "aes-gcm-siv" "0.11.1"
                "039ycyz9hijvrv2hiks9a1099yprqpkk3v39shb58dx99c9q81xf"))

(define rust-aes-kw-0.2.1
  (crate-source "aes-kw" "0.2.1"
                "131xvnah1magbr8q0lwmg3c13lv54vh41f2z79zmzyyf5lsjpyk9"))

(define rust-aho-corasick-1.1.2
  (crate-source "aho-corasick" "1.1.2"
                "1w510wnixvlgimkx1zjbvlxh6xps2vjgfqgwf5a6adlbjp5rv5mj"))

(define rust-allocator-api2-0.2.18
  (crate-source "allocator-api2" "0.2.18"
                "0kr6lfnxvnj164j1x38g97qjlhb7akppqzvgfs0697140ixbav2w"))

(define rust-android-tzdata-0.1.1
  (crate-source "android-tzdata" "0.1.1"
                "1w7ynjxrfs97xg3qlcdns4kgfpwcdv824g611fq32cag4cdr96g9"))

(define rust-android-system-properties-0.1.5
  (crate-source "android_system_properties" "0.1.5"
                "04b3wrz12837j7mdczqd95b732gw5q7q66cv4yn4646lvccp57l1"))

(define rust-anes-0.1.6
  (crate-source "anes" "0.1.6"
                "16bj1ww1xkwzbckk32j2pnbn5vk6wgsl3q4p3j9551xbcarwnijb"))

(define rust-ansi-term-0.12.1
  (crate-source "ansi_term" "0.12.1"
                "1ljmkbilxgmhavxvxqa7qvm6f3fjggi7q2l3a72q9x0cxjvrnanm"))

(define rust-anstyle-1.0.11
  (crate-source "anstyle" "1.0.11"
                "1gbbzi0zbgff405q14v8hhpi1kz2drzl9a75r3qhks47lindjbl6"))

(define rust-anyhow-1.0.99
  (crate-source "anyhow" "1.0.99"
                "001icqvkfl28rxxmk99rm4gvdzxqngj5v50yg2bh3dzcvqfllrxh"))

(define rust-approx-0.5.1
  (crate-source "approx" "0.5.1"
                "1ilpv3dgd58rasslss0labarq7jawxmivk17wsh8wmkdm3q15cfa"))

(define rust-arbitrary-1.3.2
  (crate-source "arbitrary" "1.3.2"
                "0471f0c4f1bgibhyhf8vnapkp158h1nkrzx0wnq97jwd9n0jcnkx"))

(define rust-argh-0.1.13
  (crate-source "argh" "0.1.13"
                "0h6jzj4aqswk9x6w3lbb8kdskyf93v73wlrfk4pvhdlabhr1izrl"))

(define rust-argh-derive-0.1.13
  (crate-source "argh_derive" "0.1.13"
                "00vqfqgxqq6dd9jgbg9qhn12hh06qzsj1incv3ajklsh7awb5dxd"))

(define rust-argh-shared-0.1.13
  (crate-source "argh_shared" "0.1.13"
                "1xplhinnv139x2w2wknvnms7css6c99l8dw7jb1wvv9dr0y18r54"))

(define rust-arrayref-0.3.9
  (crate-source "arrayref" "0.3.9"
                "1jzyp0nvp10dmahaq9a2rnxqdd5wxgbvp8xaibps3zai8c9fi8kn"))

(define rust-arrayvec-0.7.4
  (crate-source "arrayvec" "0.7.4"
                "04b7n722jij0v3fnm3qk072d5ysc2q30rl9fz33zpfhzah30mlwn"))

(define rust-ascii-canvas-3.0.0
  (crate-source "ascii-canvas" "3.0.0"
                "1in38ziqn4kh9sw89ys4naaqzvvjscfs0m4djqbfq7455v5fq948"))

(define rust-asn1-rs-0.5.2
  (crate-source "asn1-rs" "0.5.2"
                "1w7zq0392qs7kkv0nzw50bfqvq7q9zxv48fsp3sxyl83mzfxavvz"))

(define rust-asn1-rs-derive-0.4.0
  (crate-source "asn1-rs-derive" "0.4.0"
                "0v7fgmnzk7jjxv51grhwzcx5bf167nlqwk3vcmq7xblf5s4karbj"))

(define rust-asn1-rs-impl-0.1.0
  (crate-source "asn1-rs-impl" "0.1.0"
                "1va27bn7qxqp4wanzjlkagnynv6jnrhnwmcky2ahzb1r405p6xr7"))

(define rust-atomic-polyfill-1.0.3
  (crate-source "atomic-polyfill" "1.0.3"
                "1x00ndablb89zvbr8m03cgjzgajg86fqn8pgz85yy2gy1pivrwlc"))

(define rust-atty-0.2.14
  (crate-source "atty" "0.2.14"
                "1s7yslcs6a28c5vz7jwj63lkfgyx8mx99fdirlhi9lbhhzhrpcyr"))

(define rust-az-1.2.1
  (crate-source "az" "1.2.1"
                "0ww9k1w3al7x5qmb7f13v3s9c2pg1pdxbs8xshqy6zyrchj4qzkv"))

(define rust-base16ct-0.1.1
  (crate-source "base16ct" "0.1.1"
                "1klccxr7igf73wpi0x3asjd8n0xjg0v6a7vxgvfk5ybvgh1hd6il"))

(define rust-base32-0.4.0
  (crate-source "base32" "0.4.0"
                "1ykwx8jhksqxghfgyw2pzikzjf4n9wqm1x2ww5wqyn68ssf6dki3"))

(define rust-base45-3.1.0
  (crate-source "base45" "3.1.0"
                "0r55jzplsnl5wl4z8cxsiaiqwmzwd5kwcsh2dgl2cdznd9nky45k"))

(define rust-base64-0.13.1
  (crate-source "base64" "0.13.1"
                "1s494mqmzjb766fy1kqlccgfg2sdcjb6hzbvzqv2jw65fdi5h6wy"))

(define rust-base64-0.20.0
  (crate-source "base64" "0.20.0"
                "1r855djiv8rirg37w5arazk42ya5gm5gd2bww75v14w0sy02i8hf"))

(define rust-base64-0.21.7
  (crate-source "base64" "0.21.7"
                "0rw52yvsk75kar9wgqfwgb414kvil1gn7mqkrhn9zf1537mpsacx"))

(define rust-base64-0.22.1
  (crate-source "base64" "0.22.1"
                "1imqzgh7bxcikp5vx3shqvw9j09g9ly0xr0jma0q66i52r7jbcvj"))

(define rust-base64-0.5.2
  (crate-source "base64" "0.5.2"
                "0mvmhasrh9aqf4ksy7dbgzij435ra591a2b28v890xaf0q1krs9h"))

(define rust-base64ct-1.6.0
  (crate-source "base64ct" "1.6.0"
                "0nvdba4jb8aikv60az40x2w1y96sjdq8z3yp09rwzmkhiwv1lg4c"))

(define rust-bincode-1.3.3
  (crate-source "bincode" "1.3.3"
                "1bfw3mnwzx5g1465kiqllp5n4r10qrqy88kdlp3jfwnq2ya5xx5i"))

(define rust-bincode-2.0.0-rc.3
  (crate-source "bincode" "2.0.0-rc.3"
                "15ffn22hv950sy0x95j8r60w3bh3i835r9ili0cfz53b6jha27pi"))

(define rust-bincode-derive-2.0.0-rc.3
  (crate-source "bincode_derive" "2.0.0-rc.3"
                "0k1ygyzqpw3h5pc70cf8w4l5rv9xbk423am3lw1bi8cr7fdpac3y"))

(define rust-bit-set-0.5.3
  (crate-source "bit-set" "0.5.3"
                "1wcm9vxi00ma4rcxkl3pzzjli6ihrpn9cfdi0c5b4cvga2mxs007"))

(define rust-bit-vec-0.6.3
  (crate-source "bit-vec" "0.6.3"
                "1ywqjnv60cdh1slhz67psnp422md6jdliji6alq0gmly2xm9p7rl"))

(define rust-bitfield-struct-0.8.0
  (crate-source "bitfield-struct" "0.8.0"
                "1yqm8z33i74da2ffbd8r2283bblnmqr4cva095rr6s0wdxszh1fy"))

(define rust-bitflags-2.6.0
  (crate-source "bitflags" "2.6.0"
                "1pkidwzn3hnxlsl8zizh0bncgbjnw7c41cx7bby26ncbzmiznj5h"))

(define rust-bitmask-0.5.0
  (crate-source "bitmask" "0.5.0"
                "1bbyd12wclwz446c05bxhb7ncrdbvzwg8wx4hy91k1gmyvcv7aax"))

(define rust-bitvec-1.0.1
  (crate-source "bitvec" "1.0.1"
                "173ydyj2q5vwj88k6xgjnfsshs4x9wbvjjv7sm0h36r34hn87hhv"))

(define rust-blake2-0.10.6
  (crate-source "blake2" "0.10.6"
                "1zlf7w7gql12v61d9jcbbswa3dw8qxsjglylsiljp9f9b3a2ll26"))

(define rust-block-buffer-0.9.0
  (crate-source "block-buffer" "0.9.0"
                "1r4pf90s7d7lj1wdjhlnqa26vvbm6pnc33z138lxpnp9srpi2lj1"))

(define rust-block-padding-0.3.3
  (crate-source "block-padding" "0.3.3"
                "14wdad0r1qk5gmszxqd8cky6vx8qg7c153jv981mixzrpzmlz2d8"))

(define rust-blowfish-0.9.1
  (crate-source "blowfish" "0.9.1"
                "1mw7bvj3bg5w8vh9xw9xawqh7ixk2xwsxkj34ph96b9b1z6y44p4"))

(define rust-build-const-0.2.2
  (crate-source "build_const" "0.2.2"
                "1dryhsf4vfi1plljgv069sgfr8m1rsg04qy76x36kh6swqsl5bml"))

(define rust-bumpalo-3.16.0
  (crate-source "bumpalo" "3.16.0"
                "0b015qb4knwanbdlp1x48pkb4pm57b8gidbhhhxr900q2wb6fabr"))

(define rust-byteorder-1.5.0
  (crate-source "byteorder" "1.5.0"
                "0jzncxyf404mwqdbspihyzpkndfgda450l0893pz5xj685cg5l0z"))

(define rust-byteorder-lite-0.1.0
  (crate-source "byteorder-lite" "0.1.0"
                "15alafmz4b9az56z6x7glcbcb6a8bfgyd109qc3bvx07zx4fj7wg"))

(define rust-bzip2-0.4.4
  (crate-source "bzip2" "0.4.4"
                "1y27wgqkx3k2jmh4k26vra2kqjq1qc1asww8hac3cv1zxyk1dcdx"))

(define rust-bzip2-sys-0.1.11+1.0.8
  (crate-source "bzip2-sys" "0.1.11+1.0.8"
                "1p2crnv8d8gpz5c2vlvzl0j55i3yqg5bi0kwsl1531x77xgraskk"))

(define rust-cast-0.3.0
  (crate-source "cast" "0.3.0"
                "1dbyngbyz2qkk0jn2sxil8vrz3rnpcj142y184p9l4nbl9radcip"))

(define rust-cbc-0.1.2
  (crate-source "cbc" "0.1.2"
                "19l9y9ccv1ffg6876hshd123f2f8v7zbkc4nkckqycxf8fajmd96"))

(define rust-cc-1.0.83
  (crate-source "cc" "1.0.83"
                "1l643zidlb5iy1dskc5ggqs4wqa29a02f44piczqc8zcnsq4y5zi"))

(define rust-checked-int-cast-1.0.0
  (crate-source "checked_int_cast" "1.0.0"
                "06brva5agm6g12q15f8fidz17akb85q211496p1k2qxhb9mmxk0p"))

(define rust-chrono-0.4.33
  (crate-source "chrono" "0.4.33"
                "1szr180x4srkwvmzq5ahqnf3m7yjjllfmgp7k3hsrr556l76j4wz"))

(define rust-ciborium-0.2.2
  (crate-source "ciborium" "0.2.2"
                "03hgfw4674im1pdqblcp77m7rc8x2v828si5570ga5q9dzyrzrj2"))

(define rust-ciborium-io-0.2.2
  (crate-source "ciborium-io" "0.2.2"
                "0my7s5g24hvp1rs1zd1cxapz94inrvqpdf1rslrvxj8618gfmbq5"))

(define rust-ciborium-ll-0.2.2
  (crate-source "ciborium-ll" "0.2.2"
                "1n8g4j5rwkfs3rzfi6g1p7ngmz6m5yxsksryzf5k72ll7mjknrjp"))

(define rust-cipher-0.4.4
  (crate-source "cipher" "0.4.4"
                "1b9x9agg67xq5nq879z66ni4l08m6m3hqcshk37d4is4ysd3ngvp"))

(define rust-clap-2.34.0
  (crate-source "clap" "2.34.0"
                "071q5d8jfwbazi6zhik9xwpacx5i6kb2vkzy060vhf0c3120aqd0"))

(define rust-clap-3.2.25
  (crate-source "clap" "3.2.25"
                "08vi402vfqmfj9f07c4gl6082qxgf4c9x98pbndcnwbgaszq38af"))

(define rust-clap-4.5.48
  (crate-source "clap" "4.5.48"
                "1bjz3d7bavy13ph2a6rm3c9y02ak70b195xakii7h6q2xarln4z2"))

(define rust-clap-builder-4.5.48
  (crate-source "clap_builder" "4.5.48"
                "1jaxnr7ik25r4yxgz657vm8kz62f64qmwxhplmzxz9n0lfpn9fn2"))

(define rust-clap-derive-3.2.25
  (crate-source "clap_derive" "3.2.25"
                "025hh66cyjk5xhhq8s1qw5wkxvrm8hnv5xwwksax7dy8pnw72qxf"))

(define rust-clap-lex-0.2.4
  (crate-source "clap_lex" "0.2.4"
                "1ib1a9v55ybnaws11l63az0jgz5xiy24jkdgsmyl7grcm3sz4l18"))

(define rust-clap-lex-0.7.5
  (crate-source "clap_lex" "0.7.5"
                "0xb6pjza43irrl99axbhs12pxq4sr8x7xd36p703j57f5i3n2kxr"))

(define rust-color-quant-1.1.0
  (crate-source "color_quant" "1.1.0"
                "12q1n427h2bbmmm1mnglr57jaz2dj9apk0plcxw7nwqiai7qjyrx"))

(define rust-codespan-reporting-0.11.1
  (crate-source "codespan-reporting" "0.11.1"
                "0vkfay0aqk73d33kh79k1kqxx06ka22894xhqi89crnc6c6jff1m"))

(define rust-console-error-panic-hook-0.1.7
  (crate-source "console_error_panic_hook" "0.1.7"
                "1g5v8s0ndycc10mdn6igy914k645pgpcl8vjpz6nvxkhyirynsm0"))

(define rust-const-oid-0.7.1
  (crate-source "const-oid" "0.7.1"
                "1wwl3cncd8p2fa54vzmghflh4nh9ml02xfbv38nf5ziifh28riz4"))

(define rust-const-oid-0.9.6
  (crate-source "const-oid" "0.9.6"
                "1y0jnqaq7p2wvspnx7qj76m7hjcqpz73qzvr9l2p9n2s51vr6if2"))

(define rust-constant-time-eq-0.3.0
  (crate-source "constant_time_eq" "0.3.0"
                "1hl0y8frzlhpr58rh8rlg4bm53ax09ikj2i5fk7gpyphvhq4s57p"))

(define rust-convert-case-0.4.0
  (crate-source "convert_case" "0.4.0"
                "03jaf1wrsyqzcaah9jf8l1iznvdw5mlsca2qghhzr9w27sddaib2"))

(define rust-core-foundation-sys-0.8.6
  (crate-source "core-foundation-sys" "0.8.6"
                "13w6sdf06r0hn7bx2b45zxsg1mm2phz34jikm6xc5qrbr6djpsh6"))

(define rust-crc-1.8.1
  (crate-source "crc" "1.8.1"
                "1sqal6gm6lbj7f45iv3rw2s9w3pvvha8v970y51s7k7mwy6m8qyn"))

(define rust-crc-3.2.1
  (crate-source "crc" "3.2.1"
                "0dnn23x68qakzc429s1y9k9y3g8fn5v9jwi63jcz151sngby9rk9"))

(define rust-crc-catalog-2.4.0
  (crate-source "crc-catalog" "2.4.0"
                "1xg7sz82w3nxp1jfn425fvn1clvbzb3zgblmxsyqpys0dckp9lqr"))

(define rust-crc32fast-1.4.2
  (crate-source "crc32fast" "1.4.2"
                "1czp7vif73b8xslr3c9yxysmh9ws2r8824qda7j47ffs9pcnjxx9"))

(define rust-criterion-0.3.6
  (crate-source "criterion" "0.3.6"
                "13yd64ah93gkbdv7qq4cr6rhgl9979jjcjk3gkhnav1b7glns7dh"))

(define rust-criterion-0.5.1
  (crate-source "criterion" "0.5.1"
                "0bv9ipygam3z8kk6k771gh9zi0j0lb9ir0xi1pc075ljg80jvcgj"))

(define rust-criterion-plot-0.4.5
  (crate-source "criterion-plot" "0.4.5"
                "0xhq0jz1603585h7xvm3s4x9irmifjliklszbzs4cda00y1cqwr6"))

(define rust-criterion-plot-0.5.0
  (crate-source "criterion-plot" "0.5.0"
                "1c866xkjqqhzg4cjvg01f8w6xc1j3j7s58rdksl52skq89iq4l3b"))

(define rust-crossbeam-0.8.4
  (crate-source "crossbeam" "0.8.4"
                "1a5c7yacnk723x0hfycdbl91ks2nxhwbwy46b8y5vyy0gxzcsdqi"))

(define rust-crossbeam-channel-0.5.11
  (crate-source "crossbeam-channel" "0.5.11"
                "16v48qdflpw3hgdik70bhsj7hympna79q7ci47rw0mlgnxsw2v8p"))

(define rust-crossbeam-deque-0.8.5
  (crate-source "crossbeam-deque" "0.8.5"
                "03bp38ljx4wj6vvy4fbhx41q8f585zyqix6pncz1mkz93z08qgv1"))

(define rust-crossbeam-epoch-0.9.18
  (crate-source "crossbeam-epoch" "0.9.18"
                "03j2np8llwf376m3fxqx859mgp9f83hj1w34153c7a9c7i5ar0jv"))

(define rust-crossbeam-queue-0.3.11
  (crate-source "crossbeam-queue" "0.3.11"
                "0d8y8y3z48r9javzj67v3p2yfswd278myz1j9vzc4sp7snslc0yz"))

(define rust-crossbeam-utils-0.8.20
  (crate-source "crossbeam-utils" "0.8.20"
                "100fksq5mm1n7zj242cclkw6yf7a4a8ix3lvpfkhxvdhbda9kv12"))

(define rust-crunchy-0.2.2
  (crate-source "crunchy" "0.2.2"
                "1dx9mypwd5mpfbbajm78xcrg5lirqk7934ik980mmaffg3hdm0bs"))

(define rust-crypto-bigint-0.4.9
  (crate-source "crypto-bigint" "0.4.9"
                "1vqprgj0aj1340w186zyspi58397ih78jsc0iydvhs6zrlilnazg"))

(define rust-csv-1.3.0
  (crate-source "csv" "1.3.0"
                "1zjrlycvn44fxd9m8nwy8x33r9ncgk0k3wvy4fnvb9rpsks4ymxc"))

(define rust-csv-core-0.1.11
  (crate-source "csv-core" "0.1.11"
                "0w7s7qa60xb054rqddpyg53xq2b29sf3rbhcl8sbdx02g4yjpyjy"))

(define rust-ctaphid-0.1.1
  (crate-source "ctaphid" "0.1.1"
                "0r6bhswwar6i8hh1qwq7592y0vacqv2nfgpxrwvkwd1bb75sx7m5"))

(define rust-ctr-0.9.2
  (crate-source "ctr" "0.9.2"
                "0d88b73waamgpfjdml78icxz45d95q7vi2aqa604b0visqdfws83"))

(define rust-darling-0.13.4
  (crate-source "darling" "0.13.4"
                "0g25pad4mhq7315mw9n4wpg8j3mwyhwvr541kgdl0aar1j2ra7d0"))

(define rust-darling-0.20.5
  (crate-source "darling" "0.20.5"
                "1f66qi1v1v6sgqpah6s3syi60ql0gpg9an4ldy9aj2zxnc26npgw"))

(define rust-darling-core-0.13.4
  (crate-source "darling_core" "0.13.4"
                "046n83f9jpszlngpjxkqi39ayzxf5a35q673c69jr1dn0ylnb7c5"))

(define rust-darling-core-0.20.5
  (crate-source "darling_core" "0.20.5"
                "1qz7y44c243mlq3jnjbsab6vkxzvqbmr1l7m8q97cp6dkfaqmr04"))

(define rust-darling-macro-0.13.4
  (crate-source "darling_macro" "0.13.4"
                "0d8q8ibmsb1yzby6vwgh2wx892jqqfv9clwhpm19rprvz1wjd5ww"))

(define rust-darling-macro-0.20.5
  (crate-source "darling_macro" "0.20.5"
                "0xsg8ja6ncw9zpf7sdfinmp459z5vi97fp3y7gcy2j91gbb4a58x"))

(define rust-data-encoding-2.5.0
  (crate-source "data-encoding" "2.5.0"
                "1rcbnwfmfxhlshzbn3r7srm3azqha3mn33yxyqxkzz2wpqcjm5ky"))

(define rust-deflate64-0.1.9
  (crate-source "deflate64" "0.1.9"
                "06scix17pa7wzzfsnhkycpcc6s04shs49cdaxx2k1sl0226jnsfs"))

(define rust-defmt-0.3.6
  (crate-source "defmt" "0.3.6"
                "15a53435jpy9jj3g49mxp94g961zslggbin2nd9f2va20wlmaf9r"))

(define rust-defmt-macros-0.3.7
  (crate-source "defmt-macros" "0.3.7"
                "1nmvni24vfrcqaaaa95ag278sfm7sdshw94mkvhi7i1ap6kwgg8q"))

(define rust-defmt-parser-0.3.4
  (crate-source "defmt-parser" "0.3.4"
                "03zpg0i6vlalw7m976z66n70s041rvwii8qn3grxgs1hwgpmyjpz"))

(define rust-der-0.5.1
  (crate-source "der" "0.5.1"
                "0p3h7nszn7jhjacpmkjrcyx5g8p3ma1qhxfy3397m7l3fdfq26b9"))

(define rust-der-0.6.1
  (crate-source "der" "0.6.1"
                "1pnl3y52m1s6srxpfrfbazf6qilzq8fgksk5dv79nxaybjk6g97i"))

(define rust-der-0.7.8
  (crate-source "der" "0.7.8"
                "070bwiyr80800h31c5zd96ckkgagfjgnrrdmz3dzg2lccsd3dypz"))

(define rust-der-parser-8.2.0
  (crate-source "der-parser" "8.2.0"
                "07mnz9y395zyxwj7nam2dbzkqdngfraxp2i7y2714dxmpbxpdmnv"))

(define rust-der-derive-0.7.2
  (crate-source "der_derive" "0.7.2"
                "0jg0y3k46bpygwc5cqha07axz5sdnsx5116g3nxf0rwrabj7rs2z"))

(define rust-deranged-0.3.11
  (crate-source "deranged" "0.3.11"
                "1d1ibqqnr5qdrpw8rclwrf1myn3wf0dygl04idf4j2s49ah6yaxl"))

(define rust-derive-arbitrary-1.3.2
  (crate-source "derive_arbitrary" "1.3.2"
                "04bnd985frl81r5sgixgpvncnnj1bfpfnd7qvdx1aahnqi9pbrv7"))

(define rust-diff-0.1.13
  (crate-source "diff" "0.1.13"
                "1j0nzjxci2zqx63hdcihkp0a4dkdmzxd7my4m7zk6cjyfy34j9an"))

(define rust-digest-0.9.0
  (crate-source "digest" "0.9.0"
                "0rmhvk33rgvd6ll71z8sng91a52rw14p0drjn1da0mqa138n1pfk"))

(define rust-dirs-next-2.0.0
  (crate-source "dirs-next" "2.0.0"
                "1q9kr151h9681wwp6is18750ssghz6j9j7qm7qi1ngcwy7mzi35r"))

(define rust-dirs-sys-next-0.1.2
  (crate-source "dirs-sys-next" "0.1.2"
                "0kavhavdxv4phzj4l0psvh55hszwnr0rcz8sxbvx20pyqi2a3gaf"))

(define rust-displaydoc-0.2.5
  (crate-source "displaydoc" "0.2.5"
                "1q0alair462j21iiqwrr21iabkfnb13d6x5w95lkdg21q2xrqdlp"))

(define rust-dlib-0.5.2
  (crate-source "dlib" "0.5.2"
                "04m4zzybx804394dnqs1blz241xcy480bdwf3w9p4k6c3l46031k"))

(define rust-downcast-rs-1.2.0
  (crate-source "downcast-rs" "1.2.0"
                "0l36kgxqd5djhqwf5abxjmgasdw8n0qsjvw3jdvhi91nj393ba4y"))

(define rust-ecdsa-0.14.8
  (crate-source "ecdsa" "0.14.8"
                "0p1wxap2s6jm06y2w3cal8dkz6p9223ir9wws70rgx8h929h2cs1"))

(define rust-ed25519-1.5.3
  (crate-source "ed25519" "1.5.3"
                "1rzydm5wd8szkddx3g55w4vm86y1ika8qp8qwckada5vf1fg7kwi"))

(define rust-ed25519-compact-1.0.16
  (crate-source "ed25519-compact" "1.0.16"
                "0b95i8r8b9p5gi5c3g0cad80lzwqvvb5lb5fdxrx0hj5c3a9g2g1"))

(define rust-ed25519-dalek-2.1.0
  (crate-source "ed25519-dalek" "2.1.0"
                "1h13qm789m9gdjl6jazss80hqi8ll37m0afwcnw23zcbqjp8wqhz"))

(define rust-elliptic-curve-0.12.3
  (crate-source "elliptic-curve" "0.12.3"
                "1lwi108mh6drw5nzqzlz7ighdba5qxdg5vmwwnw1j2ihnn58ifz7"))

(define rust-embedded-graphics-0.8.1
  (crate-source "embedded-graphics" "0.8.1"
                "1w2f4zlcpd0ivfrykirar800xay0g25yd0vd29fmfvfgza59jj86"))

(define rust-embedded-graphics-core-0.4.0
  (crate-source "embedded-graphics-core" "0.4.0"
                "0i50qw5pmj2vnz9na89a86gwsik96zcgc1r21ljmc64r3wkcv7ms"))

(define rust-embedded-hal-0.2.7
  (crate-source "embedded-hal" "0.2.7"
                "1zv6pkgg2yl0mzvh3jp326rhryqfnv4l27h78v7p7maag629i51m"))

(define rust-embedded-time-0.12.1
  (crate-source "embedded-time" "0.12.1"
                "0n7yvz1j9gb0wdyafc0zf92b5bds0b28hxnvwfzhi3f41b8v996p"))

(define rust-ena-0.14.2
  (crate-source "ena" "0.14.2"
                "1wg1l7d43vfbagizsk1bl71s8xaxly4dralipm2am70fyh666cy5"))

(define rust-enum-iterator-0.6.0
  (crate-source "enum-iterator" "0.6.0"
                "1mxq9fds22paikg2c95kxkpxych4p1n3yzhca4q7fz8rl4hn76n7"))

(define rust-enum-iterator-derive-0.6.0
  (crate-source "enum-iterator-derive" "0.6.0"
                "01pc15d8l0ayrjv7xjjx1lxw2vypvlawcvc9ax7pdp60ywqsm50y"))

(define rust-enumset-1.1.3
  (crate-source "enumset" "1.1.3"
                "0z80d7v4fih563ysg8vny8kpspk3y340v7ncwmbzn4rc8skhsv12"))

(define rust-enumset-derive-0.8.1
  (crate-source "enumset_derive" "0.8.1"
                "1bykfx8qm48payzbksna5vg1ddxbgc6a2jwn8j4g0w1dp1m6r2z0"))

(define rust-env-logger-0.7.1
  (crate-source "env_logger" "0.7.1"
                "0djx8h8xfib43g5w94r1m1mkky5spcw4wblzgnhiyg5vnfxknls4"))

(define rust-env-logger-0.9.3
  (crate-source "env_logger" "0.9.3"
                "1rq0kqpa8my6i1qcyhfqrn1g9xr5fbkwwbd42nqvlzn9qibncbm1"))

(define rust-env-logger-0.10.2
  (crate-source "env_logger" "0.10.2"
                "1005v71kay9kbz1d5907l0y7vh9qn2fqsp2yfgb8bjvin6m0bm2c"))

(define rust-errno-0.3.8
  (crate-source "errno" "0.3.8"
                "0ia28ylfsp36i27g1qih875cyyy4by2grf80ki8vhgh6vinf8n52"))

(define rust-eyre-0.6.12
  (crate-source "eyre" "0.6.12"
                "1v1a3vb9gs5zkwp4jzkcfnpg0gvyp4ifydzx37f4qy14kzcibnbw"))

(define rust-fastrand-2.0.1
  (crate-source "fastrand" "2.0.1"
                "19flpv5zbzpf0rk4x77z4zf25in0brg8l7m304d3yrf47qvwxjr5"))

(define rust-fdeflate-0.3.4
  (crate-source "fdeflate" "0.3.4"
                "0ig65nz4wcqaa3y109sh7yv155ldfyph6bs2ifmz1vad1vizx6sg"))

(define rust-ff-0.12.1
  (crate-source "ff" "0.12.1"
                "0q3imz4m3dj2cy182i20wa8kbclgj13ddfngqb2miicc6cjzq4yh"))

(define rust-ff-0.13.1
  (crate-source "ff" "0.13.1"
                "14v3bc6q24gbcjnxjfbq2dddgf4as2z2gd4mj35gjlrncpxhpdf0"))

(define rust-fiat-crypto-0.1.20
  (crate-source "fiat-crypto" "0.1.20"
                "0xvbcg6wh42q3n7294mzq5xxw8fpqsgc0d69dvm5srh1f6cgc9g8"))

(define rust-fiat-crypto-0.2.7
  (crate-source "fiat-crypto" "0.2.7"
                "03w3ic88yvdpwbz36dlm7csacz4b876mlc0nbbwbc75y7apb21y0"))

(define rust-filetime-0.2.23
  (crate-source "filetime" "0.2.23"
                "1za0sbq7fqidk8aaq9v7m9ms0sv8mmi49g6p5cphpan819q4gr0y"))

(define rust-fixedbitset-0.4.2
  (crate-source "fixedbitset" "0.4.2"
                "101v41amgv5n9h4hcghvrbfk5vrncx1jwm35rn5szv4rk55i7rqc"))

(define rust-flate2-1.0.31
  (crate-source "flate2" "1.0.31"
                "083rg629001bizy25ddhlsmb9s4a297hh1d4vv7x1fv9isz1n8bz"))

(define rust-float-cmp-0.9.0
  (crate-source "float-cmp" "0.9.0"
                "1i799ksbq7fj9rm9m82g1yqgm6xi3jnrmylddmqknmksajylpplq"))

(define rust-fnv-1.0.7
  (crate-source "fnv" "1.0.7"
                "1hc2mcqha06aibcaza94vbi81j6pr9a1bbxrxjfhc91zin8yr7iz"))

(define rust-foldhash-0.1.3
  (crate-source "foldhash" "0.1.3"
                "18in1a8mjcg43pfrdkhwzr0w988zb2bmb6sqwi07snjlkhvcc7pq"))

(define rust-form-urlencoded-1.2.1
  (crate-source "form_urlencoded" "1.2.1"
                "0milh8x7nl4f450s3ddhg57a3flcv6yq8hlkyk6fyr3mcb128dp1"))

(define rust-frunk-0.4.2
  (crate-source "frunk" "0.4.2"
                "11v242h7zjka0lckxcffn5pjgr3jzxyljy7ffr0ppy8jkssm38qi"))

(define rust-frunk-core-0.4.2
  (crate-source "frunk_core" "0.4.2"
                "1mjqnn7dclwn8d5g0mrfkg360cgn70a7mm8arx6fc1xxn3x6j95g"))

(define rust-frunk-derives-0.4.2
  (crate-source "frunk_derives" "0.4.2"
                "0blsy6aq6rbvxcc0337g15083w24s8539fmv8rwp1qan2qprkymh"))

(define rust-frunk-proc-macro-helpers-0.1.2
  (crate-source "frunk_proc_macro_helpers" "0.1.2"
                "0b1xl4cfrfai7qi5cb4h9x0967miv3dvwvnsmr1vg4ljhgflmd9m"))

(define rust-fugit-0.3.7
  (crate-source "fugit" "0.3.7"
                "1rzp49521akq49vs9m8llgmdkk08zb77rry10a7srm9797b6l60p"))

(define rust-funty-2.0.0
  (crate-source "funty" "2.0.0"
                "177w048bm0046qlzvp33ag3ghqkqw4ncpzcm5lq36gxf2lla7mg6"))

(define rust-futures-0.3.30
  (crate-source "futures" "0.3.30"
                "1c04g14bccmprwsvx2j9m2blhwrynq7vhl151lsvcv4gi0b6jp34"))

(define rust-futures-channel-0.3.30
  (crate-source "futures-channel" "0.3.30"
                "0y6b7xxqdjm9hlcjpakcg41qfl7lihf6gavk8fyqijsxhvbzgj7a"))

(define rust-futures-core-0.3.30
  (crate-source "futures-core" "0.3.30"
                "07aslayrn3lbggj54kci0ishmd1pr367fp7iks7adia1p05miinz"))

(define rust-futures-executor-0.3.30
  (crate-source "futures-executor" "0.3.30"
                "07dh08gs9vfll2h36kq32q9xd86xm6lyl9xikmmwlkqnmrrgqxm5"))

(define rust-futures-io-0.3.30
  (crate-source "futures-io" "0.3.30"
                "1hgh25isvsr4ybibywhr4dpys8mjnscw4wfxxwca70cn1gi26im4"))

(define rust-futures-macro-0.3.30
  (crate-source "futures-macro" "0.3.30"
                "1b49qh9d402y8nka4q6wvvj0c88qq91wbr192mdn5h54nzs0qxc7"))

(define rust-futures-sink-0.3.30
  (crate-source "futures-sink" "0.3.30"
                "1dag8xyyaya8n8mh8smx7x6w2dpmafg2din145v973a3hw7f1f4z"))

(define rust-futures-task-0.3.30
  (crate-source "futures-task" "0.3.30"
                "013h1724454hj8qczp8vvs10qfiqrxr937qsrv6rhii68ahlzn1q"))

(define rust-futures-util-0.3.30
  (crate-source "futures-util" "0.3.30"
                "0j0xqhcir1zf2dcbpd421kgw6wvsk0rpxflylcysn1rlp3g02r1x"))

(define rust-g2gen-1.1.0
  (crate-source "g2gen" "1.1.0"
                "0z250vm1z6y8z3ds46dkk8vf2hx0jlskqiwjhhxhf7m427wk4gnw"))

(define rust-g2p-1.1.0
  (crate-source "g2p" "1.1.0"
                "17yjyal4gprhws7r9gdg9sfjjz29q4ln81msd1d3mbyrzrpgm6hs"))

(define rust-g2poly-1.1.0
  (crate-source "g2poly" "1.1.0"
                "0b2bj9n34i523h4wrkrklhrlr9ksgnxq5h30rk2zh3ghrihv5n0g"))

(define rust-gcd-2.3.0
  (crate-source "gcd" "2.3.0"
                "06l4fib4dh4m6gazdrzzzinhvcpcfh05r4i4gzscl03vnjhqnx8x"))

(define rust-gdbstub-0.6.6
  (crate-source "gdbstub" "0.6.6"
                "0p6727mfjmf7yxcrxkl7qp5pcanqd2rg22664mlxj956n7qjpq7l"))

(define rust-gdbstub-arch-0.2.4
  (crate-source "gdbstub_arch" "0.2.4"
                "177a3p17k1qg80hskg0dybjirs5hppdp946y1nh96df4amn57jzf"))

(define rust-getrandom-0.1.16
  (crate-source "getrandom" "0.1.16"
                "1kjzmz60qx9mn615ks1akjbf36n3lkv27zfwbcam0fzmj56wphwg"))

(define rust-ghostfat-0.5.0
  (crate-source "ghostfat" "0.5.0"
                "0scyq2apcim1ra5pcl6l38x7agfpr6qjygwbj5y3d548hnym3mgf"))

(define rust-glob-0.3.0
  (crate-source "glob" "0.3.0"
                "0x25wfr7vg3mzxc9x05dcphvd3nwlcmbnxrvwcvrrdwplcrrk4cv"))

(define rust-glob-0.3.1
  (crate-source "glob" "0.3.1"
                "16zca52nglanv23q5qrwd5jinw3d3as5ylya6y1pbx47vkxvrynj"))

(define rust-group-0.12.1
  (crate-source "group" "0.12.1"
                "1ixspxqdpq0hxg0hd9s6rngrp6rll21v4jjnr7ar1lzvdhxgpysx"))

(define rust-group-0.13.0
  (crate-source "group" "0.13.0"
                "0qqs2p5vqnv3zvq9mfjkmw3qlvgqb0c3cm6p33srkh7pc9sfzygh"))

(define rust-half-1.8.2
  (crate-source "half" "1.8.2"
                "1mqbmx2m9qd4lslkb42fzgldsklhv9c4bxsc8j82r80d8m24mfza"))

(define rust-half-2.6.0
  (crate-source "half" "2.6.0"
                "1j83v0xaqvrw50ppn0g33zig0zsbdi7xiqbzgn7sd5al57nrd4a5"))

(define rust-hash32-0.2.1
  (crate-source "hash32" "0.2.1"
                "0rrbv5pc5b1vax6j6hk7zvlrpw0h6aybshxy9vbpgsrgfrc5zhxh"))

(define rust-hash32-0.3.1
  (crate-source "hash32" "0.3.1"
                "01h68z8qi5gl9lnr17nz10lay8wjiidyjdyd60kqx8ibj090pmj7"))

(define rust-hashbrown-0.12.3
  (crate-source "hashbrown" "0.12.3"
                "1268ka4750pyg2pbgsr43f0289l5zah4arir2k4igx5a8c6fg7la"))

(define rust-hashbrown-0.15.1
  (crate-source "hashbrown" "0.15.1"
                "1czsvasi3azv2079fcvbhvpisa16w6fi1mfk8zm2c5wbyqdgr6rs"))

(define rust-heapless-0.7.17
  (crate-source "heapless" "0.7.17"
                "0kwn2wzk9fnsqnwp6rqjqhvh6hfq4rh225xwqjm72b5n1ry4bind"))

(define rust-heapless-0.8.0
  (crate-source "heapless" "0.8.0"
                "1b9zpdjv4qkl2511s2c80fz16fx9in4m9qkhbaa8j73032v9xyqb"))

(define rust-heck-0.4.1
  (crate-source "heck" "0.4.1"
                "1a7mqsnycv5z4z5vnv1k34548jzmc0ajic7c1j8jsaspnhw5ql4m"))

(define rust-hermit-abi-0.1.19
  (crate-source "hermit-abi" "0.1.19"
                "0cxcm8093nf5fyn114w8vxbrbcyvv91d4015rdnlgfll7cs6gd32"))

(define rust-hermit-abi-0.3.5
  (crate-source "hermit-abi" "0.3.5"
                "1hw2bxkzyvr0rbnpj0lkasi8h8qf3lyb63hp760cn22fjqaj3inh"))

(define rust-hex-0.3.2
  (crate-source "hex" "0.3.2"
                "0xsdcjiik5j750j67zk42qdnmm4ahirk3gmkmcqgq7qls2jjcl40"))

(define rust-hex-0.4.3
  (crate-source "hex" "0.4.3"
                "0w1a4davm1lgzpamwnba907aysmlrnygbqmfis2mqjx5m552a93z"))

(define rust-hex-literal-0.3.4
  (crate-source "hex-literal" "0.3.4"
                "1q54yvyy0zls9bdrx15hk6yj304npndy9v4crn1h1vd95sfv5gby"))

(define rust-hex-literal-1.0.0
  (crate-source "hex-literal" "1.0.0"
                "0wdyyq00ahhg344sd3j0k10kv1cp2cy913696n9rck2ra52yramw"))

(define rust-hidapi-1.5.0
  (crate-source "hidapi" "1.5.0"
                "1rwrxjw0zii2xz43jbx1d9zb3mbj03xma4fpk54gf2jpnvj590br"))

(define rust-hkdf-0.12.4
  (crate-source "hkdf" "0.12.4"
                "1xxxzcarz151p1b858yn5skmhyrvn8fs4ivx5km3i1kjmnr8wpvv"))

(define rust-hmac-0.12.1
  (crate-source "hmac" "0.12.1"
                "0pmbr069sfg76z7wsssfk5ddcqd9ncp79fyz6zcm6yn115yc6jbc"))

(define rust-home-0.5.9
  (crate-source "home" "0.5.9"
                "19grxyg35rqfd802pcc9ys1q3lafzlcjcv2pl2s5q8xpyr5kblg3"))

(define rust-hoot-0.1.3
  (crate-source "hoot" "0.1.3"
                "0lp427kvvdjbyiqfdcrf9cgxii4cc2jfcvhd7vz6a3hv1zcs88nz"))

(define rust-hootbin-0.1.1
  (crate-source "hootbin" "0.1.1"
                "1cpxbk2miw5hycxqdydbdvf5jhkw42nn55abqhwimsj9is360kim"))

(define rust-http-0.2.11
  (crate-source "http" "0.2.11"
                "1fwz3mhh86h5kfnr5767jlx9agpdggclq7xsqx930fflzakb2iw9"))

(define rust-httparse-1.8.0
  (crate-source "httparse" "1.8.0"
                "010rrfahm1jss3p022fqf3j3jmm72vhn4iqhykahb9ynpaag75yq"))

(define rust-humantime-1.3.0
  (crate-source "humantime" "1.3.0"
                "0krwgbf35pd46xvkqg14j070vircsndabahahlv3rwhflpy4q06z"))

(define rust-humantime-2.2.0
  (crate-source "humantime" "2.2.0"
                "17rz8jhh1mcv4b03wnknhv1shwq2v9vhkhlfg884pprsig62l4cv"))

(define rust-iana-time-zone-0.1.60
  (crate-source "iana-time-zone" "0.1.60"
                "0hdid5xz3jznm04lysjm3vi93h3c523w0hcc3xba47jl3ddbpzz7"))

(define rust-iana-time-zone-haiku-0.1.2
  (crate-source "iana-time-zone-haiku" "0.1.2"
                "17r6jmj31chn7xs9698r122mapq85mfnv98bb4pg6spm0si2f67k"))

(define rust-ident-case-1.0.1
  (crate-source "ident_case" "1.0.1"
                "0fac21q6pwns8gh1hz3nbq15j8fi441ncl6w4vlnd1cmc55kiq5r"))

(define rust-idna-0.3.0
  (crate-source "idna" "0.3.0"
                "1rh9f9jls0jy3g8rh2bfpjhvvhh4q80348jc4jr2s844133xykg1"))

(define rust-image-0.25.5
  (crate-source "image" "0.25.5"
                "0fsnfgg8hr66ag5nxipvb7d50kbg40qfpbsql59qkwa2ssp48vyd"))

(define rust-indenter-0.3.3
  (crate-source "indenter" "0.3.3"
                "10y6i6y4ls7xsfsc1r3p5j2hhbxhaqnk5zzk8aj52b14v05ba8yf"))

(define rust-indexmap-1.9.3
  (crate-source "indexmap" "1.9.3"
                "16dxmy7yvk51wvnih3a3im6fp5lmx0wx76i03n06wyak6cwhw1xx"))

(define rust-inout-0.1.3
  (crate-source "inout" "0.1.3"
                "1xf9gf09nc7y1a261xlfqsf66yn6mb81ahlzzyyd1934sr9hbhd0"))

(define rust-instant-0.1.12
  (crate-source "instant" "0.1.12"
                "0b2bx5qdlwayriidhrag8vhy10kdfimfhmb3jnjmsz2h9j1bwnvs"))

(define rust-is-terminal-0.4.10
  (crate-source "is-terminal" "0.4.10"
                "0m9la3f7cs77y85nkbcjsxkb7k861fc6bdhahyfidgh7gljh1b8b"))

(define rust-itertools-0.10.5
  (crate-source "itertools" "0.10.5"
                "0ww45h7nxx5kj6z2y6chlskxd1igvs4j507anr6dzg99x1h25zdh"))

(define rust-itm-logger-0.1.2
  (crate-source "itm_logger" "0.1.2"
                "003kmmc7qpmadya39pivkxr936gnwl7kqsw08qzyq7iwk3xlmj69"))

(define rust-itoa-1.0.1
  (crate-source "itoa" "1.0.1"
                "0d8wr2qf5b25a04xf10rz9r0pdbjdgb0zaw3xvf8k2sqcz1qzaqs"))

(define rust-itoa-1.0.10
  (crate-source "itoa" "1.0.10"
                "0k7xjfki7mnv6yzjrbnbnjllg86acmbnk4izz2jmm1hx2wd6v95i"))

(define rust-jobserver-0.1.28
  (crate-source "jobserver" "0.1.28"
                "1mji1wis4w76v3issgpah2x3j1k0ybq0cz3qgypg7pkdablscimb"))

(define rust-js-sys-0.3.68
  (crate-source "js-sys" "0.3.68"
                "1vm98fhnhs4w6yakchi9ip7ar95900k9vkr24a21qlwd6r5xlv20"))

(define rust-keccak-0.1.5
  (crate-source "keccak" "0.1.5"
                "0m06swsyd58hvb1z17q6picdwywprf1yf1s6l491zi8r26dazhpc"))

(define rust-lalrpop-0.19.12
  (crate-source "lalrpop" "0.19.12"
                "0yw3m7br8zsby1vb7d0v952hdllg6splc85ba4l9yn1746avy70a"))

(define rust-lalrpop-util-0.19.12
  (crate-source "lalrpop-util" "0.19.12"
                "1vd0iy505h97xxm66r3m68a34v0009784syy093mlk30p4vq5i6k"))

(define rust-libc-0.2.174
  (crate-source "libc" "0.2.174"
                "0xl7pqvw7g2874dy3kjady2fjr4rhj5lxsnxkkhr5689jcr6jw8i"))

(define rust-libloading-0.8.1
  (crate-source "libloading" "0.8.1"
                "0q812zvfag4m803ak640znl6cf8ngdd0ilzky498r6pwvmvbcwf5"))

(define rust-libm-0.1.4
  (crate-source "libm" "0.1.4"
                "16pc0gx4gkg0q2s1ssq8268brn14j8344623vwhadmivc4lsmivz"))

(define rust-libm-0.2.8
  (crate-source "libm" "0.2.8"
                "0n4hk1rs8pzw8hdfmwn96c4568s93kfxqgcqswr7sajd2diaihjf"))

(define rust-libredox-0.0.1
  (crate-source "libredox" "0.0.1"
                "1s2fh4ikpp9xl0lsl01pi0n8pw1q9s3ld452vd8qh1v63v537j45"))

(define rust-libredox-0.0.2
  (crate-source "libredox" "0.0.2"
                "01v6pb09j7dl2gnbvzz6zmy2k4zyxjjzvl7wacwjjffqsxajry9s"))

(define rust-linux-raw-sys-0.4.13
  (crate-source "linux-raw-sys" "0.4.13"
                "172k2c6422gsc914ig8rh99mb9yc7siw6ikc3d9xw1k7vx0s3k81"))

(define rust-lockfree-object-pool-0.1.6
  (crate-source "lockfree-object-pool" "0.1.6"
                "0bjm2g1g1avab86r02jb65iyd7hdi35khn1y81z4nba0511fyx4k"))

(define rust-lru-0.12.5
  (crate-source "lru" "0.12.5"
                "0f1a7cgqxbyhrmgaqqa11m3azwhcc36w0v5r4izgbhadl3sg8k13"))

(define rust-lzma-rs-0.3.0
  (crate-source "lzma-rs" "0.3.0"
                "0phif4pnjrn28zcxgz3a7z86hhx5gdajmkrndfw4vrkahd682zi9"))

(define rust-managed-0.8.0
  (crate-source "managed" "0.8.0"
                "13b1j5gpm55jxk24qrbpc25j0ds47bkk9g83d04kp50ab9r8va0c"))

(define rust-memoffset-0.6.5
  (crate-source "memoffset" "0.6.5"
                "1kkrzll58a3ayn5zdyy9i1f1v3mx0xgl29x0chq614zazba638ss"))

(define rust-merlin-2.0.1
  (crate-source "merlin" "2.0.1"
                "0hivklid2gzwz6179g0wiay55ah3xafvaavxkznjvi5kz3q1q9jf"))

(define rust-merlin-3.0.0
  (crate-source "merlin" "3.0.0"
                "0z9rh9jlpcs0i0cijbs6pcq26gl4qwz05y7zbnv7h2gwk4kqxhsq"))

(define rust-micromath-2.1.0
  (crate-source "micromath" "2.1.0"
                "05g8zavgsks2f1rkl8fd8lxsbmb51yjls88phwijyfph9yjdvj63"))

(define rust-minifb-0.26.0
  (crate-source "minifb" "0.26.0"
                "1yc0p462bvq52pdh2g429whikjq3dhdvjycgiqyr1pjq280c7sin"))

(define rust-minimal-lexical-0.2.1
  (crate-source "minimal-lexical" "0.2.1"
                "16ppc5g84aijpri4jzv14rvcnslvlpphbszc7zzp6vfkddf4qdb8"))

(define rust-miniz-oxide-0.4.4
  (crate-source "miniz_oxide" "0.4.4"
                "0jsfv00hl5rmx1nijn59sr9jmjd4rjnjhh4kdjy8d187iklih9d9"))

(define rust-miniz-oxide-0.7.2
  (crate-source "miniz_oxide" "0.7.2"
                "19qlxb21s6kabgqq61mk7kd1qk2invyygj076jz6i1gj2lz1z0cx"))

(define rust-nalgebra-0.33.2
  (crate-source "nalgebra" "0.33.2"
                "0fvayv2fa6x4mfm4cq3m2cfcc2jwkiq4sm73209zszkh9gvcvbi6"))

(define rust-nb-0.1.3
  (crate-source "nb" "0.1.3"
                "0vyh31pbwrg21f8hz1ipb9i20qwnfwx47gz92i9frdhk0pd327c0"))

(define rust-nb-1.1.0
  (crate-source "nb" "1.1.0"
                "179kbn9l6vhshncycagis7f8mfjppz4fhvgnmcikqz30mp23jm4d"))

(define rust-new-debug-unreachable-1.0.4
  (crate-source "new_debug_unreachable" "1.0.4"
                "0m1bg3wz3nvxdryg78x4i8hh9fys4wp2bi0zg821dhvf44v4g8p4"))

(define rust-nix-0.24.3
  (crate-source "nix" "0.24.3"
                "0sc0yzdl51b49bqd9l9cmimp1sw1hxb8iyv4d35ww6d7m5rfjlps"))

(define rust-no-std-net-0.6.0
  (crate-source "no-std-net" "0.6.0"
                "0ravflgyh0q2142gjdz9iav5yqci3ga7gbnk4mmfcnqkrq54lya3"))

(define rust-nom-7.1.3
  (crate-source "nom" "7.1.3"
                "0jha9901wxam390jcf5pfa0qqfrgh8li787jx2ip0yk5b8y9hwyj"))

(define rust-nu-ansi-term-0.46.0
  (crate-source "nu-ansi-term" "0.46.0"
                "115sywxh53p190lyw97alm14nc004qj5jm5lvdj608z84rbida3p"))

(define rust-num-0.3.1
  (crate-source "num" "0.3.1"
                "13vsnqr0kasn7rwfq5r1vqdd0sy0y5ar3x4xhvzy4fg0wndqwylb"))

(define rust-num-bigint-0.4.4
  (crate-source "num-bigint" "0.4.4"
                "1h6d8pd0h7grpva2pa78i7lhvl69kqdq156qcaicpmy3nmcpd3k0"))

(define rust-num-complex-0.3.1
  (crate-source "num-complex" "0.3.1"
                "1igjwm5kk2df9mxmpb260q6p40xfnkrq4smymgdqg2sm1hn66zbl"))

(define rust-num-complex-0.4.6
  (crate-source "num-complex" "0.4.6"
                "15cla16mnw12xzf5g041nxbjjm9m85hdgadd5dl5d0b30w9qmy3k"))

(define rust-num-conv-0.1.0
  (crate-source "num-conv" "0.1.0"
                "1ndiyg82q73783jq18isi71a7mjh56wxrk52rlvyx0mi5z9ibmai"))

(define rust-num-derive-0.3.3
  (crate-source "num-derive" "0.3.3"
                "0gbl94ckzqjdzy4j8b1p55mz01g6n1l9bckllqvaj0wfz7zm6sl7"))

(define rust-num-integer-0.1.46
  (crate-source "num-integer" "0.1.46"
                "13w5g54a9184cqlbsq80rnxw4jj4s0d8wv75jsq5r2lms8gncsbr"))

(define rust-num-iter-0.1.44
  (crate-source "num-iter" "0.1.44"
                "1aamy25jyys9rh6qmadmr9f3hd9qz7qr407xcd0jhmf4q0fc0sfq"))

(define rust-num-rational-0.3.2
  (crate-source "num-rational" "0.3.2"
                "01sgiwny9iflyxh2xz02sak71v2isc3x608hfdpwwzxi3j5l5b0j"))

(define rust-num-rational-0.4.2
  (crate-source "num-rational" "0.4.2"
                "093qndy02817vpgcqjnj139im3jl7vkq4h68kykdqqh577d18ggq"))

(define rust-num-cpus-1.16.0
  (crate-source "num_cpus" "1.16.0"
                "0hra6ihpnh06dvfvz9ipscys0xfqa9ca9hzp384d5m02ssvgqqa1"))

(define rust-num-enum-0.6.1
  (crate-source "num_enum" "0.6.1"
                "18bna04g6zq978z2b4ygz0f8pbva37id4xnpgwh8l41w1m1mn0bs"))

(define rust-num-enum-derive-0.6.1
  (crate-source "num_enum_derive" "0.6.1"
                "19k57c0wg56vzzj2w77jsi8nls1b8xh8pvpzjnrgf8d9cnvpsrln"))

(define rust-oid-registry-0.6.1
  (crate-source "oid-registry" "0.6.1"
                "1zwvjp3ad6gzn8g8w2hcn9a2xdap0lkzckhlnwp6rabbzdpz7vcv"))

(define rust-oorandom-11.1.3
  (crate-source "oorandom" "11.1.3"
                "0xdm4vd89aiwnrk1xjwzklnchjqvib4klcihlc2bsd4x50mbrc8a"))

(define rust-opaque-debug-0.3.0
  (crate-source "opaque-debug" "0.3.0"
                "1m8kzi4nd6shdqimn0mgb24f0hxslhnqd1whakyq06wcqd086jk2"))

(define rust-option-block-0.3.0
  (crate-source "option-block" "0.3.0"
                "09rjxxz4zj3i4fbsbf9fkw2z0mbr8f7sccmhr3bi8sjr8p9wbwp0"))

(define rust-orbclient-0.3.47
  (crate-source "orbclient" "0.3.47"
                "0rk144mqpv27r390bjn6dfcp2314xxfila6g3njx6x4pvr5xbw2j"))

(define rust-os-str-bytes-6.6.1
  (crate-source "os_str_bytes" "6.6.1"
                "1885z1x4sm86v5p41ggrl49m58rbzzhd1kj72x46yy53p62msdg2"))

(define rust-overload-0.1.1
  (crate-source "overload" "0.1.1"
                "0fdgbaqwknillagy1xq7xfgv60qdbk010diwl7s1p0qx7hb16n5i"))

(define rust-p256-0.11.1
  (crate-source "p256" "0.11.1"
                "151mqd8m25c8ib97saz4fwkg4nhw098i051gazg2l7pm13flxx2i"))

(define rust-packed-simd-2-0.3.8
  (crate-source "packed_simd_2" "0.3.8"
                "10p2bm0p57shg3arlpfwm6z0bbnlkyr4g0dlkmpwvz6qaba4r4d1"))

(define rust-packed-struct-0.10.1
  (crate-source "packed_struct" "0.10.1"
                "14617sfax8rigzkcagmw97m3pxrxnrrq89w2nbwfzj9c8f8rdcin"))

(define rust-packed-struct-codegen-0.10.1
  (crate-source "packed_struct_codegen" "0.10.1"
                "164z2y7xp2wy41dawrp94g6slky46k0157m0d87kxmahzrnp1mlw"))

(define rust-packing-0.1.0
  (crate-source "packing" "0.1.0"
                "1fbwbiih5m2y00hkb8zkzd98afiwhvlik4fx7k697k4zsmf6zijl"))

(define rust-packing-0.2.0
  (crate-source "packing" "0.2.0"
                "1kdy7wqrj049azzn2h0g56d0sg6cvi4s317643703a212q8g6i22"))

(define rust-packing-codegen-0.1.0
  (crate-source "packing_codegen" "0.1.0"
                "0vsl6lbdjp62n6bcvcc3qbzip90fb40lmq263nzq3mhw70jym860"))

(define rust-parking-lot-0.12.1
  (crate-source "parking_lot" "0.12.1"
                "13r2xk7mnxfc5g0g6dkdxqdqad99j7s7z8zhzz4npw5r0g0v4hip"))

(define rust-parking-lot-core-0.9.9
  (crate-source "parking_lot_core" "0.9.9"
                "13h0imw1aq86wj28gxkblhkzx6z1gk8q18n0v76qmmj6cliajhjc"))

(define rust-passwords-3.1.16
  (crate-source "passwords" "3.1.16"
                "176mr017icfz736c7j6bbm6pmkzxlr6kkhqfdgn19gf2ly9p2h0i"))

(define rust-pbkdf2-0.12.2
  (crate-source "pbkdf2" "0.12.2"
                "1wms79jh4flpy1zi8xdp4h8ccxv4d85adc6zjagknvppc5vnmvgq"))

(define rust-pem-0.8.3
  (crate-source "pem" "0.8.3"
                "1sqkzp87j6s79sjxk4n913gcmalzb2fdc75l832d0j7a3z9cnmpx"))

(define rust-pem-rfc7468-0.3.1
  (crate-source "pem-rfc7468" "0.3.1"
                "0c7vrrksg8fqzxb7q4clzl14f0qnqky7jqspjqi4pailiybmvph1"))

(define rust-pem-rfc7468-0.7.0
  (crate-source "pem-rfc7468" "0.7.0"
                "04l4852scl4zdva31c1z6jafbak0ni5pi0j38ml108zwzjdrrcw8"))

(define rust-percent-encoding-2.3.1
  (crate-source "percent-encoding" "2.3.1"
                "0gi8wgx0dcy8rnv1kywdv98lwcx67hz0a0zwpib5v2i08r88y573"))

(define rust-petgraph-0.6.4
  (crate-source "petgraph" "0.6.4"
                "1ac6wfq5f5pzcv0nvzzfgjbwg2kwslpnzsw5wcmxlscfcb9azlz1"))

(define rust-phf-shared-0.10.0
  (crate-source "phf_shared" "0.10.0"
                "15n02nc8yqpd8hbxngblar2g53p3nllc93d8s8ih3p5cf7bnlydn"))

(define rust-pin-project-lite-0.2.13
  (crate-source "pin-project-lite" "0.2.13"
                "0n0bwr5qxlf0mhn2xkl36sy55118s9qmvx2yl5f3ixkb007lbywa"))

(define rust-pin-utils-0.1.0
  (crate-source "pin-utils" "0.1.0"
                "117ir7vslsl2z1a7qzhws4pd01cg2d3338c47swjyvqv2n60v1wb"))

(define rust-pio-0.2.1
  (crate-source "pio" "0.2.1"
                "1qvq03nbx6vjix7spr5fcxcbxw39flm1y72kxl1g728gnna9dq3n"))

(define rust-pio-parser-0.2.2
  (crate-source "pio-parser" "0.2.2"
                "0syxm3rnjgcicn5wqv67dimksnla54ayy1vjzj6zkbkrh8mjqlvp"))

(define rust-pio-proc-0.2.2
  (crate-source "pio-proc" "0.2.2"
                "0yhdks1c9rf2ng90as5ng3mw92mwacdwda2c7s5zv95k1y3xq13b"))

(define rust-pkcs8-0.10.2
  (crate-source "pkcs8" "0.10.2"
                "1dx7w21gvn07azszgqd3ryjhyphsrjrmq5mmz1fbxkj5g0vv4l7r"))

(define rust-pkcs8-0.8.0
  (crate-source "pkcs8" "0.8.0"
                "1l29h4mrgi2kpsl98jzky3ni5by3xa1sc6db9yd8l1i1p0zxmavw"))

(define rust-pkcs8-0.9.0
  (crate-source "pkcs8" "0.9.0"
                "1fm4sigvcd0zpzg9jcp862a8p272kk08b9lgcs1dm1az19cjrjly"))

(define rust-pkg-config-0.3.29
  (crate-source "pkg-config" "0.3.29"
                "1jy6158v1316khkpmq2sjj1vgbnbnw51wffx7p0k0l9h9vlys019"))

(define rust-plotters-0.3.5
  (crate-source "plotters" "0.3.5"
                "0igxq58bx96gz58pqls6g3h80plf17rfl3b6bi6xvjnp02x29hnj"))

(define rust-plotters-backend-0.3.5
  (crate-source "plotters-backend" "0.3.5"
                "02cn98gsj2i1bwrfsymifmyas1wn2gibdm9mk8w82x9s9n5n4xly"))

(define rust-plotters-svg-0.3.5
  (crate-source "plotters-svg" "0.3.5"
                "1axbw82frs5di4drbyzihr5j35wpy2a75hp3f49p186cjfcd7xiq"))

(define rust-png-0.17.11
  (crate-source "png" "0.17.11"
                "0nnss6y2la7bq2lj1y8z2bxkigk6c2l9bzx2irdnd5bmc4z3qv0z"))

(define rust-png-decoder-0.1.1
  (crate-source "png-decoder" "0.1.1"
                "0i2rc4bszzk0xazq9avjp2dyhjawwb9spbybvkr0j9jai3a96giy"))

(define rust-polyval-0.6.1
  (crate-source "polyval" "0.6.1"
                "1yr6x3r776038bsxn40akqbl22ii9aghja9ps3k5zvjd3nfzyb6m"))

(define rust-powerfmt-0.2.0
  (crate-source "powerfmt" "0.2.0"
                "14ckj2xdpkhv3h6l5sdmb9f1d57z8hbfpdldjc2vl5givq2y77j3"))

(define rust-precomputed-hash-0.1.1
  (crate-source "precomputed-hash" "0.1.1"
                "075k9bfy39jhs53cb2fpb9klfakx2glxnf28zdw08ws6lgpq6lwj"))

(define rust-proc-macro-error-1.0.4
  (crate-source "proc-macro-error" "1.0.4"
                "1373bhxaf0pagd8zkyd03kkx6bchzf6g0dkwrwzsnal9z47lj9fs"))

(define rust-proc-macro-error-attr-1.0.4
  (crate-source "proc-macro-error-attr" "1.0.4"
                "0sgq6m5jfmasmwwy8x4mjygx5l7kp8s4j60bv25ckv2j1qc41gm1"))

(define rust-proc-macro-hack-0.5.20+deprecated
  (crate-source "proc-macro-hack" "0.5.20+deprecated"
                "0s402hmcs3k9nd6rlp07zkr1lz7yimkmcwcbgnly2zr44wamwdyw"))

(define rust-protobuf-3.7.2
  (crate-source "protobuf" "3.7.2"
                "1x4riz4znnjsqpdxnhxj0aq8rfivmbv4hfqmd3gbbn77v96isnnn"))

(define rust-protobuf-codegen-3.7.2
  (crate-source "protobuf-codegen" "3.7.2"
                "1kjaakqk0595akxdhv68w23zw136hw0h0kxkyg9bn500bj17cfax"))

(define rust-protobuf-parse-3.7.2
  (crate-source "protobuf-parse" "3.7.2"
                "0wy9pnfrsk2iz2ghhvzdpp0riklrm6p8dvdfxr4d7wb04hgsmbml"))

(define rust-protobuf-support-3.7.2
  (crate-source "protobuf-support" "3.7.2"
                "1mnpn2q96bxm2vidh86m5p2x5z0z8rgfyixk1wlgjiqa3vrw4diy"))

(define rust-ptr-meta-0.1.4
  (crate-source "ptr_meta" "0.1.4"
                "1wd4wy0wxrcays4f1gy8gwcmxg7mskmivcv40p0hidh6xbvwqf07"))

(define rust-ptr-meta-derive-0.1.4
  (crate-source "ptr_meta_derive" "0.1.4"
                "1b69cav9wn67cixshizii0q5mlbl0lihx706vcrzm259zkdlbf0n"))

(define rust-qrcode-0.12.0
  (crate-source "qrcode" "0.12.0"
                "0zzmrwb44r17zn0hkpin0yldwxjdwya2nkvv23jwcc1nbx2z3lhn"))

(define rust-quick-error-1.2.3
  (crate-source "quick-error" "1.2.3"
                "1q6za3v78hsspisc197bg3g7rpc989qycy8ypr8ap8igv10ikl51"))

(define rust-r0-1.0.0
  (crate-source "r0" "1.0.0"
                "04gjrcvl56x45jiz5awggxkz8wlfj1hp3bcjbp4wn7ars7p32ymx"))

(define rust-radium-0.7.0
  (crate-source "radium" "0.7.0"
                "02cxfi3ky3c4yhyqx9axqwhyaca804ws46nn4gc1imbk94nzycyw"))

(define rust-rand-0.7.3
  (crate-source "rand" "0.7.3"
                "00sdaimkbz491qgi6qxkv582yivl32m2jd401kzbn94vsiwicsva"))

(define rust-rand-0.8.5
  (crate-source "rand" "0.8.5"
                "013l6931nn7gkc23jz5mm3qdhf93jjf0fg64nz2lp4i51qd8vbrl"))

;; rand 0.6.x for usb-device git dependency
(define rust-rand-0.6.5
  (crate-source "rand" "0.6.5"
                "1jl4449jcl4wgmzld6ffwqj5gwxrp8zvx8w573g1z368qg6xlwbd"))

(define rust-rand-chacha-0.1.1
  (crate-source "rand_chacha" "0.1.1"
                "1vxwyzs4fy1ffjc8l00fsyygpiss135irjf7nyxgq2v0lqf3lvam"))

(define rust-rand-core-0.4.2
  (crate-source "rand_core" "0.4.2"
                "1p09ynysrq1vcdlmcqnapq4qakl2yd1ng3kxh3qscpx09k2a6cww"))

(define rust-rand-core-0.3.1
  (crate-source "rand_core" "0.3.1"
                "0jzdgszfa4bliigiy4hi66k7fs3gfwi2qxn8vik84ph77fwdwvvs"))

(define rust-rand-hc-0.1.0
  (crate-source "rand_hc" "0.1.0"
                "1i0vl8q5ddvvy0x8hf1zxny393miyzxkwqnw31ifg6p0gdy6fh3v"))

(define rust-rand-isaac-0.1.1
  (crate-source "rand_isaac" "0.1.1"
                "027flpjr4znx2csxk7gxb7vrf9c7y5mydmvg5az2afgisp4rgnfy"))

(define rust-rand-jitter-0.1.4
  (crate-source "rand_jitter" "0.1.4"
                "16z387y46bfz3csc42zxbjq89vcr1axqacncvv8qhyy93p4xarhi"))

(define rust-rand-pcg-0.1.2
  (crate-source "rand_pcg" "0.1.2"
                "0i0bdla18a8x4jn1w0fxsbs3jg7ajllz6azmch1zw33r06dv1ydb"))

(define rust-rand-xorshift-0.1.1
  (crate-source "rand_xorshift" "0.1.1"
                "0p2x8nr00hricpi2m6ca5vysiha7ybnghz79yqhhx6sl4gkfkxyb"))

(define rust-rand-os-0.1.3
  (crate-source "rand_os" "0.1.3"
                "0wahppm0s64gkr2vmhcgwc0lij37in1lgfxg5rbgqlz0l5vgcxbv"))

(define rust-rand-chacha-0.2.2
  (crate-source "rand_chacha" "0.2.2"
                "00il36fkdbsmpr99p9ksmmp6dn1md7rmnwmz0rr77jbrca2yvj7l"))

(define rust-rand-core-0.5.1
  (crate-source "rand_core" "0.5.1"
                "06bdvx08v3rkz451cm7z59xwwqn1rkfh6v9ay77b14f8dwlybgch"))

(define rust-rand-hc-0.2.0
  (crate-source "rand_hc" "0.2.0"
                "0g31sqwpmsirdlwr0svnacr4dbqyz339im4ssl9738cjgfpjjcfa"))

(define rust-rand-xorshift-0.3.0
  (crate-source "rand_xorshift" "0.3.0"
                "13vcag7gmqspzyabfl1gr9ykvxd2142q2agrj8dkyjmfqmgg4nyj"))

(define rust-rdrand-0.4.0
  (crate-source "rdrand" "0.4.0"
                "1cjq0kwx1bk7jx3kzyciiish5gqsj7620dm43dc52sr8fzmm9037"))

(define rust-random-number-0.1.8
  (crate-source "random-number" "0.1.8"
                "1gmp9ca9m20bgijswag3z68csdslfkjafm1sq1852z62nk5sag9s"))

(define rust-random-number-macro-impl-0.1.7
  (crate-source "random-number-macro-impl" "0.1.7"
                "02qsbyx9xgmcawc9vasv1iaard6macf5q5p7bmnckkqwyhn2k1lb"))

(define rust-random-pick-1.2.16
  (crate-source "random-pick" "1.2.16"
                "1y50la3p7cwn06cmkxvfz71f4b81lr55yz8j8kz9ly6sfa84jyf1"))

(define rust-raw-window-handle-0.6.1
  (crate-source "raw-window-handle" "0.6.1"
                "12s1ck4v5ib1zclasr348sxpb76cnkk6hag603ki3z6xn6yvrhwc"))

(define rust-rayon-1.8.1
  (crate-source "rayon" "1.8.1"
                "0lg0488xwpj5jsfz2gfczcrpclbjl8221mj5vdrhg8bp3883fwps"))

(define rust-rayon-core-1.12.1
  (crate-source "rayon-core" "1.12.1"
                "1qpwim68ai5h0j7axa8ai8z0payaawv3id0lrgkqmapx7lx8fr8l"))

(define rust-redox-syscall-0.4.1
  (crate-source "redox_syscall" "0.4.1"
                "1aiifyz5dnybfvkk4cdab9p2kmphag1yad6iknc7aszlxxldf8j7"))

(define rust-redox-users-0.4.4
  (crate-source "redox_users" "0.4.4"
                "1d1c7dhbb62sh8jrq9dhvqcyxqsh3wg8qknsi94iwq3r0wh7k151"))

(define rust-regex-1.10.3
  (crate-source "regex" "1.10.3"
                "05cvihqy0wgnh9i8a9y2n803n5azg2h0b7nlqy6rsvxhy00vwbdn"))

(define rust-regex-automata-0.4.5
  (crate-source "regex-automata" "0.4.5"
                "1karc80mx15z435rm1jg3sqylnc58nxi15gqypcd1inkzzpqgfav"))

(define rust-regex-syntax-0.6.29
  (crate-source "regex-syntax" "0.6.29"
                "1qgj49vm6y3zn1hi09x91jvgkl2b1fiaq402skj83280ggfwcqpi"))

(define rust-regex-syntax-0.8.2
  (crate-source "regex-syntax" "0.8.2"
                "17rd2s8xbiyf6lb4aj2nfi44zqlj98g2ays8zzj2vfs743k79360"))

(define rust-rfc6979-0.2.0
  (crate-source "rfc6979" "0.2.0"
                "1plmmpcazvn3l5ddzdpqcb4xrz64xfa3a7grkb217qaygm1qh1vc"))

(define rust-rfc6979-0.3.1
  (crate-source "rfc6979" "0.3.1"
                "1fzsp705b5lhwd2r9il9grc3lj6rm3b2r89vh0xv181gy5xg2hvp"))

(define rust-rkyv-0.4.3
  (crate-source "rkyv" "0.4.3"
                "041h7n493rpdnv2rpacf7ncjdcyjjrq7ffykrm7bmfp7iyrh3pkh"))

(define rust-rkyv-derive-0.4.0
  (crate-source "rkyv_derive" "0.4.0"
                "051zhzx3wslfdqybklkk1cb0h9hf2psd0fgdhqz070aspkv6k8cm"))

(define rust-rust-fuzzy-search-0.1.1
  (crate-source "rust-fuzzy-search" "0.1.1"
                "1chvl47hq42r219yxs6r1dp4l19acy5ay145hpc5drgzaiq6amx1"))

(define rust-rustc-std-workspace-alloc-1.0.0
  (crate-source "rustc-std-workspace-alloc" "1.0.0"
                "11psmqk6glglxl3zwh8slz6iynfxaifh4spd2wcnws552dqdarpz"))

(define rust-rusticata-macros-4.1.0
  (crate-source "rusticata-macros" "4.1.0"
                "0ch67lljmgl5pfrlb90bl5kkp2x6yby1qaxnpnd0p5g9xjkc9w7s"))

(define rust-rustix-0.38.31
  (crate-source "rustix" "0.38.31"
                "0jg9yj3i6qnzk1y82hng7rb1bwhslfbh57507dxcs9mgcakf38vf"))

(define rust-rustls-0.22.2
  (crate-source "rustls" "0.22.2"
                "0hcxyhq6ynvws9v5b2h81s1nwmijmya7a3vyyyhsy1wqpmb9jz78"))

(define rust-rustls-pki-types-1.2.0
  (crate-source "rustls-pki-types" "1.2.0"
                "1kxsl7dkjjmb5hpq9as54zhbs9vf45axi4yd2w7fjn1ibsv6ww8a"))

(define rust-rustls-webpki-0.102.1
  (crate-source "rustls-webpki" "0.102.1"
                "0nz9d3xhy8cg6anmvq64scyiva8bglrc6j3v6bdxw2f96xha4k7g"))

(define rust-rustversion-1.0.14
  (crate-source "rustversion" "1.0.14"
                "1x1pz1yynk5xzzrazk2svmidj69jhz89dz5vrc28sixl20x1iz3z"))

;; rusb for usb-device git dependency
(define rust-rusb-0.8.1
  (crate-source "rusb" "0.8.1"
                "1b80icrc7amkg1mz1cwi4hprslfcw1g3w2vm3ixyfnyc5130i9fr"))

(define rust-rusb-0.9.4
  (crate-source "rusb" "0.9.4"
                "1905rijhabvylblh24379229hjmkfhxr80jc79aqd9v3bgq9z7xb"))

(define rust-libusb1-sys-0.6.4
  (crate-source "libusb1-sys" "0.6.4"
                "09sznaf1lkahb6rfz2j0zbrcm2viz1d1wl8qlk4z4ia2rspy5l7r"))

(define rust-libusb1-sys-0.7.0
  (crate-source "libusb1-sys" "0.7.0"
                "03yfx469d1ldpw2h21hy322f5a0h1ahlgy4s6yjipzy4gbg0l1fs"))

(define rust-libusb1-sys-0.5.0
  (crate-source "libusb1-sys" "0.5.0"
                "0gq27za2av9gvdz1pgwlzaw3bflyhlxj0inlqp31cs5yig88jbp2"))

(define rust-ryu-1.0.16
  (crate-source "ryu" "1.0.16"
                "0k7b90xr48ag5bzmfjp82rljasw2fx28xr3bg1lrpx7b5sljm3gr"))

(define rust-same-file-1.0.6
  (crate-source "same-file" "1.0.6"
                "00h5j1w87dmhnvbv9l8bic3y7xxsnjmssvifw2ayvgx9mb1ivz4k"))

(define rust-scoped-tls-1.0.1
  (crate-source "scoped-tls" "1.0.1"
                "15524h04mafihcvfpgxd8f4bgc3k95aclz8grjkg9a0rxcvn9kz1"))

(define rust-sct-0.7.1
  (crate-source "sct" "0.7.1"
                "056lmi2xkzdg1dbai6ha3n57s18cbip4pnmpdhyljli3m99n216s"))

(define rust-sdl2-0.35.2
  (crate-source "sdl2" "0.35.2"
                "06ivcavxhc7zyhbfmy2544dz0lnaqf33d9xf0jggpw93nrvr55gp"))

(define rust-sdl2-sys-0.35.2
  (crate-source "sdl2-sys" "0.35.2"
                "1w7ranfpmbvsnviq0y8d1cz9pajp8c4b84lslycq02kcrzi6nn73"))

(define rust-sec1-0.3.0
  (crate-source "sec1" "0.3.0"
                "0a09lk5w3nyggpyz54m10nnlg9v8qbh6kw3v1bgla31988c4rqiv"))

(define rust-serde-1.0.215
  (crate-source "serde" "1.0.215"
                "13xqkw93cw9rnbkm0zy1apnilzq7l2xf1qw8m1nkga8i1fnw24v5"))

(define rust-serde-bytes-0.11.14
  (crate-source "serde_bytes" "0.11.14"
                "0d0pb7wsq2nszxvg2dmzbj9wsvrzchbq2m4742csnhzx2g1rg14b"))

(define rust-serde-cbor-0.11.2
  (crate-source "serde_cbor" "0.11.2"
                "1xf1bq7ixha30914pd5jl3yw9v1x6car7xgrpimvfvs5vszjxvrb"))

(define rust-serde-derive-1.0.215
  (crate-source "serde_derive" "1.0.215"
                "1h2nswy0rmzblil38h12wxsgni1ik63rk22wy19g48v9hrpqc7md"))

(define rust-serde-json-1.0.113
  (crate-source "serde_json" "1.0.113"
                "0ycaiff7ar4qx5sy9kvi1kv9rnnfl15kcfmhxiiwknn3n5q1p039"))

(define rust-serde-repr-0.1.20
  (crate-source "serde_repr" "0.1.20"
                "1755gss3f6lwvv23pk7fhnjdkjw7609rcgjlr8vjg6791blf6php"))

(define rust-serde-spanned-0.6.9
  (crate-source "serde_spanned" "0.6.9"
                "18vmxq6qfrm110caszxrzibjhy2s54n1g5w1bshxq9kjmz7y0hdz"))

(define rust-serde-with-1.14.0
  (crate-source "serde_with" "1.14.0"
                "1zqjlc9ypm8y0r9bcgdhh62zcdn2yzfxh31dsbn01gshkq35m2v7"))

(define rust-serde-with-macros-1.5.2
  (crate-source "serde_with_macros" "1.5.2"
                "10l0rsy0k61nvpn1brcfvzp8yfnvsqdgh6zdwp03qf85dzndd0p1"))

(define rust-sha1-0.10.6
  (crate-source "sha1" "0.10.6"
                "1fnnxlfg08xhkmwf2ahv634as30l1i3xhlhkvxflmasi5nd85gz3"))

(define rust-sha3-0.10.8
  (crate-source "sha3" "0.10.8"
                "0q5s3qlwnk8d5j34jya98j1v2p3009wdmnqdza3yydwgi8kjv1vm"))

(define rust-sharded-slab-0.1.7
  (crate-source "sharded-slab" "0.1.7"
                "1xipjr4nqsgw34k7a2cgj9zaasl2ds6jwn89886kww93d32a637l"))

(define rust-signature-1.6.4
  (crate-source "signature" "1.6.4"
                "0z3xg405pg827g6hfdprnszsdqkkbrsfx7f1dl04nv9g7cxks8vl"))

(define rust-simba-0.9.0
  (crate-source "simba" "0.9.0"
                "1yp0dfi2wgw0xkihfpav65hr52zym8bsw59ck2blf46d06jqd8xk"))

(define rust-simd-adler32-0.3.7
  (crate-source "simd-adler32" "0.3.7"
                "1zkq40c3iajcnr5936gjp9jjh1lpzhy44p3dq3fiw75iwr1w2vfn"))

(define rust-siphasher-0.3.11
  (crate-source "siphasher" "0.3.11"
                "03axamhmwsrmh0psdw3gf7c0zc4fyl5yjxfifz9qfka6yhkqid9q"))

(define rust-slab-0.4.9
  (crate-source "slab" "0.4.9"
                "0rxvsgir0qw5lkycrqgb1cxsvxzjv9bmx73bk5y42svnzfba94lg"))

(define rust-smallvec-1.13.1
  (crate-source "smallvec" "1.13.1"
                "1mzk9j117pn3k1gabys0b7nz8cdjsx5xc6q7fwnm8r0an62d7v76"))

(define rust-smoltcp-0.11.0
  (crate-source "smoltcp" "0.11.0"
                "15ycgk4ds8x2qi2l41ikm5q0sj41lc2zmh68l9qmj2z5a5lrj6js"))

(define rust-sntpc-0.3.7
  (crate-source "sntpc" "0.3.7"
                "09356ilpvf37lpd2gnfkf0ahn8iiwk4frph2qwa8lyrxz0w9z6k2"))

(define rust-spin-0.9.8
  (crate-source "spin" "0.9.8"
                "0rvam5r0p3a6qhc18scqpvpgb3ckzyqxpgdfyjnghh8ja7byi039"))

(define rust-spki-0.5.4
  (crate-source "spki" "0.5.4"
                "09qaddm4kw01xm9638910bm4yqnshzh2p38lvc3kxkvc5b01ml24"))

(define rust-spki-0.6.0
  (crate-source "spki" "0.6.0"
                "0ar1ldkl7svp8l3gfw2hyiiph7n2nqynjnjgdv1pscvsmjxh5kv7"))

(define rust-spki-0.7.3
  (crate-source "spki" "0.7.3"
                "17fj8k5fmx4w9mp27l970clrh5qa7r5sjdvbsln987xhb34dc7nr"))

(define rust-stable-deref-trait-1.2.0
  (crate-source "stable_deref_trait" "1.2.0"
                "1lxjr8q2n534b2lhkxd6l6wcddzjvnksi58zv11f9y0jjmr15wd8"))

(define rust-stats-alloc-0.1.10
  (crate-source "stats_alloc" "0.1.10"
                "1v2ys8m1737nz4h9ahwkajgz0mqs9hhbkfx19iqnjgkk9r1083jw"))

(define rust-string-cache-0.8.7
  (crate-source "string_cache" "0.8.7"
                "0fr90a54ibsrnfjq5la77yjd641g6vqv8f1v3pmpbxa2cbkkh4gr"))

(define rust-strsim-0.10.0
  (crate-source "strsim" "0.10.0"
                "08s69r4rcrahwnickvi0kq49z524ci50capybln83mg6b473qivk"))

(define rust-strsim-0.8.0
  (crate-source "strsim" "0.8.0"
                "0sjsm7hrvjdifz661pjxq5w4hf190hx53fra8dfvamacvff139cf"))

(define rust-svd2utra-0.1.15
  (crate-source "svd2utra" "0.1.15"
                "190c3hf066b3ld2p4bw17r18xac6id79mn97966yhj3zkcymgfrg"))

(define rust-synstructure-0.12.6
  (crate-source "synstructure" "0.12.6"
                "03r1lydbf3japnlpc4wka7y90pmz1i0danaj3f9a7b431akdlszk"))

(define rust-tap-1.0.1
  (crate-source "tap" "1.0.1"
                "0sc3gl4nldqpvyhqi3bbd0l9k7fngrcl4zs47n314nqqk4bpx4sm"))

(define rust-tempfile-3.10.0
  (crate-source "tempfile" "3.10.0"
                "0rwycrln0gkapm931zy2zq3l3l2w9d7jhzyqjppn4iz4336yhrd3"))

(define rust-term-0.7.0
  (crate-source "term" "0.7.0"
                "07xzxmg7dbhlirpyfq09v7cfb9gxn0077sqqvszgjvyrjnngi7f5"))

(define rust-termcolor-1.4.1
  (crate-source "termcolor" "1.4.1"
                "0mappjh3fj3p2nmrg4y7qv94rchwi9mzmgmfflr8p2awdj7lyy86"))

(define rust-textwrap-0.11.0
  (crate-source "textwrap" "0.11.0"
                "0q5hky03ik3y50s9sz25r438bc4nwhqc6dqwynv4wylc807n29nk"))

(define rust-textwrap-0.16.2
  (crate-source "textwrap" "0.16.2"
                "0mrhd8q0dnh5hwbwhiv89c6i41yzmhw4clwa592rrp24b9hlfdf1"))

(define rust-thiserror-1.0.63
  (crate-source "thiserror" "1.0.63"
                "092p83mf4p1vkjb2j6h6z96dan4raq2simhirjv12slbndq26d60"))

(define rust-thiserror-impl-1.0.63
  (crate-source "thiserror-impl" "1.0.63"
                "0qd21l2jjrkvnpr5da3l3b58v4wmrkn6aa0h1z5dg6kb8rc8nmd4"))

(define rust-thread-local-1.1.7
  (crate-source "thread_local" "1.1.7"
                "0lp19jdgvp5m4l60cgxdnl00yw1hlqy8gcywg9bddwng9h36zp9z"))

(define rust-threadpool-1.8.1
  (crate-source "threadpool" "1.8.1"
                "1amgfyzvynbm8pacniivzq9r0fh3chhs7kijic81j76l6c5ycl6h"))

(define rust-time-0.3.36
  (crate-source "time" "0.3.36"
                "11g8hdpahgrf1wwl2rpsg5nxq3aj7ri6xr672v4qcij6cgjqizax"))

(define rust-time-core-0.1.2
  (crate-source "time-core" "0.1.2"
                "1wx3qizcihw6z151hywfzzyd1y5dl804ydyxci6qm07vbakpr4pg"))

(define rust-time-macros-0.2.18
  (crate-source "time-macros" "0.2.18"
                "1kqwxvfh2jkpg38fy673d6danh1bhcmmbsmffww3mphgail2l99z"))

(define rust-tiny-keccak-2.0.2
  (crate-source "tiny-keccak" "2.0.2"
                "0dq2x0hjffmixgyf6xv9wgsbcxkd65ld0wrfqmagji8a829kg79c"))

(define rust-tinytemplate-1.2.1
  (crate-source "tinytemplate" "1.2.1"
                "1g5n77cqkdh9hy75zdb01adxn45mkh9y40wdr7l68xpz35gnnkdy"))

(define rust-tock-registers-0.8.1
  (crate-source "tock-registers" "0.8.1"
                "077jq2lhq1qkg0cxlsrxbk2j4pgx31wv6y59cnhpdqp7msh42sb9"))

(define rust-toml-0.5.11
  (crate-source "toml" "0.5.11"
                "0d2266nx8b3n22c7k24x4428z6di8n83a9n466jm7a2hipfz1xzl"))

(define rust-toml-0.7.8
  (crate-source "toml" "0.7.8"
                "0mr2dpmzw4ndvzpnnli2dprcx61pdk62fq4mzw0b6zb27ffycyfx"))

(define rust-toml-datetime-0.6.11
  (crate-source "toml_datetime" "0.6.11"
                "077ix2hb1dcya49hmi1avalwbixmrs75zgzb3b2i7g2gizwdmk92"))

(define rust-toml-edit-0.19.15
  (crate-source "toml_edit" "0.19.15"
                "08bl7rp5g6jwmfpad9s8jpw8wjrciadpnbaswgywpr9hv9qbfnqv"))

(define rust-tracing-0.1.40
  (crate-source "tracing" "0.1.40"
                "1vv48dac9zgj9650pg2b4d0j3w6f3x9gbggf43scq5hrlysklln3"))

(define rust-tracing-attributes-0.1.27
  (crate-source "tracing-attributes" "0.1.27"
                "1rvb5dn9z6d0xdj14r403z0af0bbaqhg02hq4jc97g5wds6lqw1l"))

(define rust-tracing-core-0.1.32
  (crate-source "tracing-core" "0.1.32"
                "0m5aglin3cdwxpvbg6kz0r9r0k31j48n0kcfwsp6l49z26k3svf0"))

(define rust-tracing-log-0.2.0
  (crate-source "tracing-log" "0.2.0"
                "1hs77z026k730ij1a9dhahzrl0s073gfa2hm5p0fbl0b80gmz1gf"))

(define rust-tracing-subscriber-0.3.18
  (crate-source "tracing-subscriber" "0.3.18"
                "12vs1bwk4kig1l2qqjbbn2nm5amwiqmkcmnznylzmnfvjy6083xd"))

(define rust-tracking-allocator-0.3.0
  (crate-source "tracking-allocator" "0.3.0"
                "0wjybg3wkpkl51z11la6kgy53z75prqkxrwviprgyr92mygiw85a"))

(define rust-tungstenite-0.20.1
  (crate-source "tungstenite" "0.20.1"
                "1fbgcv3h4h1bhhf5sqbwqsp7jnc44bi4m41sgmhzdsk2zl8aqgcy"))

(define rust-uf2-block-0.1.0
  (crate-source "uf2_block" "0.1.0"
                "0jrsbyc5n91rspvgb7b6d4gbzwydirwq99gilx4yzkbblly7kpv8"))

(define rust-unicode-bidi-0.3.15
  (crate-source "unicode-bidi" "0.3.15"
                "0xcdxm7h0ydyprwpcbh436rbs6s6lph7f3gr527lzgv6lw053y88"))

(define rust-unicode-normalization-0.1.22
  (crate-source "unicode-normalization" "0.1.22"
                "08d95g7b1irc578b2iyhzv4xhsa4pfvwsqxcl9lbcpabzkq16msw"))

(define rust-unicode-width-0.1.11
  (crate-source "unicode-width" "0.1.11"
                "11ds4ydhg8g7l06rlmh712q41qsrd0j0h00n1jm74kww3kqk65z5"))

(define rust-unicode-xid-0.2.4
  (crate-source "unicode-xid" "0.2.4"
                "131dfzf7d8fsr1ivch34x42c2d1ik5ig3g78brxncnn0r1sdyqpr"))

(define rust-universal-hash-0.5.1
  (crate-source "universal-hash" "0.5.1"
                "1sh79x677zkncasa95wz05b36134822w6qxmi1ck05fwi33f47gw"))

(define rust-untrusted-0.7.1
  (crate-source "untrusted" "0.7.1"
                "0jkbqaj9d3v5a91pp3wp9mffvng1nhycx6sh4qkdd9qyr62ccmm1"))

(define rust-untrusted-0.9.0
  (crate-source "untrusted" "0.9.0"
                "1ha7ib98vkc538x0z60gfn0fc5whqdd85mb87dvisdcaifi6vjwf"))

(define rust-ureq-2.9.5
  (crate-source "ureq" "2.9.5"
                "0kf6vhyb355rjdkq8xs9shl6q79nxs505m49hb8jzfyn0cfp6lhb"))

(define rust-url-2.3.1
  (crate-source "url" "2.3.1"
                "0hs67jw257y0a7mj2p9wi0n61x8fc2vgwxg37y62nxkmmscwfs0d"))

(define rust-usbd-serial-0.1.1
  (crate-source "usbd-serial" "0.1.1"
                "1zhxksam5kngqh574fwzzr4r30yc9v7wfwfiy3f14zr8hsdm2xfv"))

(define rust-usbd-bulk-only-transport-0.1.0
  (crate-source "usbd_bulk_only_transport" "0.1.0"
                "0pqk29g17jppkpx13riksc01bx7v9wz9jprsarwvq8gqwwqxkz56"))

(define rust-usbd-mass-storage-0.1.0
  (crate-source "usbd_mass_storage" "0.1.0"
                "1ifycziv55lzvqvffwph1qqc3dmmvsjsrz99x8k2yy2i5cnrm79d"))

(define rust-usbd-scsi-0.1.0
  (crate-source "usbd_scsi" "0.1.0"
                "0iw6bf1r3kg57fp0d6bys424pqgkbadcrffh3bj68gxijjdxlyvw"))

(define rust-utf-8-0.7.6
  (crate-source "utf-8" "0.7.6"
                "1a9ns3fvgird0snjkd3wbdhwd3zdpc2h5gpyybrfr6ra5pkqxk09"))

(define rust-valuable-0.1.0
  (crate-source "valuable" "0.1.0"
                "0v9gp3nkjbl30z0fd56d8mx7w1csk86wwjhfjhr400wh9mfpw2w3"))

(define rust-vcell-0.1.3
  (crate-source "vcell" "0.1.3"
                "00n0ss2z3rh0ihig6d4w7xp72g58f7g1m6s5v4h3nc6jacdrqhvp"))

(define rust-vec-map-0.8.2
  (crate-source "vec_map" "0.8.2"
                "1481w9g1dw9rxp3l6snkdqihzyrd2f8vispzqmwjwsdyhw8xzggi"))

(define rust-version-compare-0.1.1
  (crate-source "version-compare" "0.1.1"
                "0acg4pmjdbmclg0m7yhijn979mdy66z3k8qrcnvn634f1gy456jp"))

(define rust-virtue-0.0.13
  (crate-source "virtue" "0.0.13"
                "051k8yr55j0iq28xcmr9jsj7vlri28ah9w8f5b479xsdcb061k4x"))

(define rust-void-1.0.2
  (crate-source "void" "1.0.2"
                "0zc8f0ksxvmhvgx4fdg0zyn6vdnbxd2xv9hfx4nhzg6kbs4f80ka"))

(define rust-walkdir-2.4.0
  (crate-source "walkdir" "2.4.0"
                "1vjl9fmfc4v8k9ald23qrpcbyb8dl1ynyq8d516cm537r1yqa7fp"))

(define rust-wasi-0.11.0+wasi-snapshot-preview1
  (crate-source "wasi" "0.11.0+wasi-snapshot-preview1"
                "08z4hxwkpdpalxjps1ai9y7ihin26y9f476i53dv98v45gkqg3cw"))

(define rust-wasi-0.9.0+wasi-snapshot-preview1
  (crate-source "wasi" "0.9.0+wasi-snapshot-preview1"
                "06g5v3vrdapfzvfq662cij7v8a1flwr2my45nnncdv2galrdzkfc"))

(define rust-wasm-bindgen-0.2.91
  (crate-source "wasm-bindgen" "0.2.91"
                "0zwbb07ln4m5hh6axamc701nnj090nd66syxbf6bagzf189j9qf1"))

(define rust-wasm-bindgen-backend-0.2.91
  (crate-source "wasm-bindgen-backend" "0.2.91"
                "02zpi9sjzhd8kfv1yj9m1bs4a41ik9ii5bc8hjf60arm1j8f3ry9"))

(define rust-wasm-bindgen-futures-0.4.41
  (crate-source "wasm-bindgen-futures" "0.4.41"
                "15zd36y0jpzvh18x963hd905rlpk2cxp918r6db0xsnfc4zrqyw7"))

(define rust-wasm-bindgen-macro-0.2.91
  (crate-source "wasm-bindgen-macro" "0.2.91"
                "1va6dilw9kcnvsg5043h5b9mwc5sgq0lyhj9fif2n62qsgigj2mk"))

(define rust-wasm-bindgen-macro-support-0.2.91
  (crate-source "wasm-bindgen-macro-support" "0.2.91"
                "0rlyl3yzwbcnc691mvx78m1wbqf1qs52mlc3g88bh7ihwrdk4bv4"))

(define rust-wasm-bindgen-shared-0.2.91
  (crate-source "wasm-bindgen-shared" "0.2.91"
                "0f4qmjv57ppwi4xpdxgcd77vz9vmvlrnybg8dj430hzhvk96n62g"))

(define rust-wasm-bindgen-test-0.3.41
  (crate-source "wasm-bindgen-test" "0.3.41"
                "0qgbv1fh8bsvs1vqvlpja877pz4bw638jq9f4l6yvqikz2sdwg8l"))

(define rust-wasm-bindgen-test-macro-0.3.41
  (crate-source "wasm-bindgen-test-macro" "0.3.41"
                "12bgbvygyi04d1gcrgl7w7m94mn7is59f7ds5cqmfs30a1sin8d5"))

(define rust-wayland-client-0.29.5
  (crate-source "wayland-client" "0.29.5"
                "05b7qikqj22rjy17kqw5ar7j2chpy18dr0gqapvwjfd00n60cfrz"))

(define rust-wayland-commons-0.29.5
  (crate-source "wayland-commons" "0.29.5"
                "00m90bnxqy0d6lzqlyazc1jh18jgbjwigmyr0rk3m8w4slsg34c6"))

(define rust-wayland-cursor-0.29.5
  (crate-source "wayland-cursor" "0.29.5"
                "0qbn6wqmjibkx3lb3ggbp07iabzgx2zhrm0wxxxjbmhkdyvccrb8"))

(define rust-wayland-protocols-0.29.5
  (crate-source "wayland-protocols" "0.29.5"
                "1ihbjyd0w460gd7w22g9qabbwd4v8x74f8vsh7p25csljcgn4l5r"))

(define rust-wayland-scanner-0.29.5
  (crate-source "wayland-scanner" "0.29.5"
                "0lxx3i2kxnmsk421qx87lqqc9kd2y1ksjxcyg0pqbar2zbc06hwg"))

(define rust-wayland-sys-0.29.5
  (crate-source "wayland-sys" "0.29.5"
                "1m79qqmr1hx7jlyrvnrxjma5s6dk5js9fjsr4nx7vv1r7hdcw4my"))

(define rust-web-sys-0.3.68
  (crate-source "web-sys" "0.3.68"
                "0il4nbsf782l5y1jb7s75vc7214a19vh7z65bfrwwykzd03mjmln"))

(define rust-webpki-roots-0.26.0
  (crate-source "webpki-roots" "0.26.0"
                "1221q07j5sv23bmwv8my49hdax70dwzdpsnjgrdbw88gk3dczqhd"))

(define rust-which-4.4.2
  (crate-source "which" "4.4.2"
                "1ixzmx3svsv5hbdvd8vdhd3qwvf6ns8jdpif1wmwsy10k90j9fl7"))

(define rust-winapi-0.3.9
  (crate-source "winapi" "0.3.9"
                "06gl025x418lchw1wxj64ycr7gha83m44cjr5sarhynd9xkrm0sw"))

(define rust-winapi-i686-pc-windows-gnu-0.4.0
  (crate-source "winapi-i686-pc-windows-gnu" "0.4.0"
                "1dmpa6mvcvzz16zg6d5vrfy4bxgg541wxrcip7cnshi06v38ffxc"))

(define rust-winapi-util-0.1.6
  (crate-source "winapi-util" "0.1.6"
                "15i5lm39wd44004i9d5qspry2cynkrpvwzghr6s2c3dsk28nz7pj"))

(define rust-winapi-x86-64-pc-windows-gnu-0.4.0
  (crate-source "winapi-x86_64-pc-windows-gnu" "0.4.0"
                "0gqq64czqb64kskjryj8isp62m2sgvx25yyj3kpc2myh85w24bki"))

(define rust-windows-core-0.52.0
  (crate-source "windows-core" "0.52.0"
                "1nc3qv7sy24x0nlnb32f7alzpd6f72l4p24vl65vydbyil669ark"))

(define rust-windows-sys-0.48.0
  (crate-source "windows-sys" "0.48.0"
                "1aan23v5gs7gya1lc46hqn9mdh8yph3fhxmhxlw36pn6pqc28zb7"))

(define rust-windows-sys-0.52.0
  (crate-source "windows-sys" "0.52.0"
                "0gd3v4ji88490zgb6b5mq5zgbvwv7zx1ibn8v3x83rwcdbryaar8"))

(define rust-windows-targets-0.48.5
  (crate-source "windows-targets" "0.48.5"
                "034ljxqshifs1lan89xwpcy1hp0lhdh4b5n0d2z4fwjx2piacbws"))

(define rust-windows-targets-0.52.0
  (crate-source "windows-targets" "0.52.0"
                "1kg7a27ynzw8zz3krdgy6w5gbqcji27j1sz4p7xk2j5j8082064a"))

(define rust-windows-aarch64-gnullvm-0.48.5
  (crate-source "windows_aarch64_gnullvm" "0.48.5"
                "1n05v7qblg1ci3i567inc7xrkmywczxrs1z3lj3rkkxw18py6f1b"))

(define rust-windows-aarch64-gnullvm-0.52.0
  (crate-source "windows_aarch64_gnullvm" "0.52.0"
                "1shmn1kbdc0bpphcxz0vlph96bxz0h1jlmh93s9agf2dbpin8xyb"))

(define rust-windows-aarch64-msvc-0.48.5
  (crate-source "windows_aarch64_msvc" "0.48.5"
                "1g5l4ry968p73g6bg6jgyvy9lb8fyhcs54067yzxpcpkf44k2dfw"))

(define rust-windows-aarch64-msvc-0.52.0
  (crate-source "windows_aarch64_msvc" "0.52.0"
                "1vvmy1ypvzdvxn9yf0b8ygfl85gl2gpcyvsvqppsmlpisil07amv"))

(define rust-windows-i686-gnu-0.48.5
  (crate-source "windows_i686_gnu" "0.48.5"
                "0gklnglwd9ilqx7ac3cn8hbhkraqisd0n83jxzf9837nvvkiand7"))

(define rust-windows-i686-gnu-0.52.0
  (crate-source "windows_i686_gnu" "0.52.0"
                "04zkglz4p3pjsns5gbz85v4s5aw102raz4spj4b0lmm33z5kg1m2"))

(define rust-windows-i686-msvc-0.48.5
  (crate-source "windows_i686_msvc" "0.48.5"
                "01m4rik437dl9rdf0ndnm2syh10hizvq0dajdkv2fjqcywrw4mcg"))

(define rust-windows-i686-msvc-0.52.0
  (crate-source "windows_i686_msvc" "0.52.0"
                "16kvmbvx0vr0zbgnaz6nsks9ycvfh5xp05bjrhq65kj623iyirgz"))

(define rust-windows-x86-64-gnu-0.48.5
  (crate-source "windows_x86_64_gnu" "0.48.5"
                "13kiqqcvz2vnyxzydjh73hwgigsdr2z1xpzx313kxll34nyhmm2k"))

(define rust-windows-x86-64-gnu-0.52.0
  (crate-source "windows_x86_64_gnu" "0.52.0"
                "1zdy4qn178sil5sdm63lm7f0kkcjg6gvdwmcprd2yjmwn8ns6vrx"))

(define rust-windows-x86-64-gnullvm-0.48.5
  (crate-source "windows_x86_64_gnullvm" "0.48.5"
                "1k24810wfbgz8k48c2yknqjmiigmql6kk3knmddkv8k8g1v54yqb"))

(define rust-windows-x86-64-gnullvm-0.52.0
  (crate-source "windows_x86_64_gnullvm" "0.52.0"
                "17lllq4l2k1lqgcnw1cccphxp9vs7inq99kjlm2lfl9zklg7wr8s"))

(define rust-windows-x86-64-msvc-0.48.5
  (crate-source "windows_x86_64_msvc" "0.48.5"
                "0f4mdp895kkjh9zv8dxvn4pc10xr7839lf5pa9l0193i2pkgr57d"))

(define rust-windows-x86-64-msvc-0.52.0
  (crate-source "windows_x86_64_msvc" "0.52.0"
                "012wfq37f18c09ij5m6rniw7xxn5fcvrxbqd0wd8vgnl3hfn9yfz"))

(define rust-winnow-0.5.40
  (crate-source "winnow" "0.5.40"
                "0xk8maai7gyxda673mmw3pj1hdizy5fpi7287vaywykkk19sk4zm"))

(define rust-wyz-0.5.1
  (crate-source "wyz" "0.5.1"
                "1vdrfy7i2bznnzjdl9vvrzljvs4s3qm8bnlgqwln6a941gy61wq5"))

(define rust-x11-dl-2.21.0
  (crate-source "x11-dl" "2.21.0"
                "0vsiq62xpcfm0kn9zjw5c9iycvccxl22jya8wnk18lyxzqj5jwrq"))

(define rust-x25519-dalek-2.0.1
  (crate-source "x25519-dalek" "2.0.1"
                "0xyjgqpsa0q6pprakdp58q1hy45rf8wnqqscgzx0gyw13hr6ir67"))

(define rust-x509-parser-0.15.1
  (crate-source "x509-parser" "0.15.1"
                "1nk3ryam7yzsza735xdypkv1i4c35gqlygax5jyr74bbnsjznsbh"))

(define rust-xcursor-0.3.5
  (crate-source "xcursor" "0.3.5"
                "0499ff2gy9hfb9dvndn5zyc7gzz9lhc5fly3s3yfsiak99xws33a"))

(define rust-xmas-elf-0.9.1
  (crate-source "xmas-elf" "0.9.1"
                "1inias7h1cv4zh3szk46byiqhnzm5zc7658q1brzfhl3wwbrii22"))

(define rust-xml-rs-0.8.19
  (crate-source "xml-rs" "0.8.19"
                "0nnpvk3fv32hgh7vs9gbg2swmzxx5yz73f4b7rak7q39q2x9rjqg"))

(define rust-xous-api-log-0.1.68
  (crate-source "xous-api-log" "0.1.68"
                "0im49nln08kcykdpcbb6f1fndxgvrq3i282i1asn6f4yf1ndf3vv"))

(define rust-xous-api-names-0.9.70
  (crate-source "xous-api-names" "0.9.70"
                "1cwm9l9ync8hlddwzj0hngfwrqv5scjmya7r6l2gbncf7h68gw21"))

(define rust-xous-api-susres-0.9.68
  (crate-source "xous-api-susres" "0.9.68"
                "1c2irky7sl5nngxh7ifiwxjpxilx8ymap5slydx630pfm8p2av0i"))

(define rust-xous-api-ticktimer-0.9.69
  (crate-source "xous-api-ticktimer" "0.9.69"
                "1c2rbg2jda6ic6yvdlrl1dnq9k402jf0zh03lq69cynhjj241gqh"))

(define rust-xous-ipc-0.10.9
  (crate-source "xous-ipc" "0.10.9"
                "1xmdlml3kmfwkxbq8vgjz0gp14q9yf30n9mpzymd62wc7l20svji"))

(define rust-xous-ipc-0.9.63
  (crate-source "xous-ipc" "0.9.63"
                "0d6wv15zb2z6vcn52l61f4519bc5dyfvm3mgahsc0d9xyic83j27"))

(define rust-xous-semver-0.1.5
  (crate-source "xous-semver" "0.1.5"
                "1hn8djhal1wrqcba5pysww4bxgrp0gaf8hw77ky3mlaf1jz00rch"))

(define rust-xous-tts-backend-0.1.6
  (crate-source "xous-tts-backend" "0.1.6"
                "1g6f7hkca1gs1yprs6yy2gjxvwmn70ap7cwwlm5wagmgxhig2cr1"))

(define rust-zero-0.1.3
  (crate-source "zero" "0.1.3"
                "113pa9jj40x6bvxsw582ca9np7d53qkb2b6cavfyczya6k61pqig"))

(define rust-zeroize-derive-1.4.2
  (crate-source "zeroize_derive" "1.4.2"
                "0sczjlqjdmrp3wn62g7mw6p438c9j4jgp2f9zamd56991mdycdnf"))

(define rust-zip-2.1.6
  (crate-source "zip" "2.1.6"
                "0biy7mxqnzaibz603jmly52gzvyvqmbndlgvw5n2i5n2xy98rpa0"))

(define rust-zopfli-0.8.1
  (crate-source "zopfli" "0.8.1"
                "0ip9azz9ldk19m0m1hdppz3n5zcz0cywbg1vx59g4p5c3cwry0g5"))

(define rust-zstd-0.13.2
  (crate-source "zstd" "0.13.2"
                "1ygkr6wspm9clbp7ykyl0rv69cfsf9q4lic9wcqiwn34lrwbgwpw"))

(define rust-zstd-safe-7.2.1
  (crate-source "zstd-safe" "7.2.1"
                "0nch85m5cr493y26yvndm6a8j6sd9mxpr2awrim3dslcnr6sp8sl"))

(define rust-zstd-sys-2.0.13+zstd.1.5.6
  (crate-source "zstd-sys" "2.0.13+zstd.1.5.6"
                "1almbackh06am0d2kc4a089n3al91jg3ahgg9kcrg3zfrwhhzzrq"))

;;;
;;; Crate input lists for packages
;;;

(define bao1x-boot0-crate-inputs
  (list
   rust-adler-1.0.2
   rust-aead-0.5.2
   rust-aes-gcm-siv-0.11.1
   rust-aes-kw-0.2.1
   rust-aho-corasick-0.7.18  ; for locales
   rust-aho-corasick-1.1.2
   rust-allocator-api2-0.2.18
   rust-android-system-properties-0.1.5
   rust-android-tzdata-0.1.1
   rust-anes-0.1.6
   rust-ansi-term-0.12.1
   rust-anstyle-1.0.11
   rust-anyhow-1.0.99
   rust-approx-0.5.1
   rust-arbitrary-1.3.2
   rust-arbitrary-int-1.3.0
   rust-argh-0.1.13
   rust-argh-derive-0.1.13
   rust-argh-shared-0.1.13
   rust-arrayref-0.3.9
   rust-arrayvec-0.7.4
   rust-ascii-canvas-3.0.0
   rust-asn1-rs-0.5.2
   rust-asn1-rs-derive-0.4.0
   rust-asn1-rs-impl-0.1.0
   rust-atomic-polyfill-1.0.3
   rust-atty-0.2.14
   rust-autocfg-1.1.0
   rust-autocfg-0.1.8
   rust-az-1.2.1
   rust-bare-metal-0.2.4
   rust-base16ct-0.1.1
   rust-base32-0.4.0
   rust-base45-3.1.0
   rust-base64-0.13.1
   rust-base64-0.20.0
   rust-base64-0.21.7
   rust-base64-0.22.1
   rust-base64-0.5.2
   rust-base64ct-1.6.0
   rust-bincode-1.3.3
   rust-bincode-2.0.0-rc.3
   rust-bincode-derive-2.0.0-rc.3
   rust-bitbybit-1.4.0
   rust-bitfield-0.13.2
   rust-bit-field-0.9.0
   rust-bitfield-struct-0.8.0
   rust-bitflags-1.3.2
   rust-bitflags-2.6.0
   rust-bitmask-0.5.0
   rust-bit-set-0.5.3
   rust-bit-vec-0.6.3
   rust-bitvec-1.0.1
   rust-blake2-0.10.6
   rust-block-buffer-0.10.4
   rust-block-buffer-0.9.0
   rust-block-padding-0.3.3
   rust-blowfish-0.9.1
   rust-build-const-0.2.2
   rust-bumpalo-3.16.0
   rust-bytemuck-1.24.0
   rust-bytemuck-derive-1.10.2
   rust-byteorder-1.5.0
   rust-byteorder-lite-0.1.0
   rust-bytes-1.5.0
   rust-bzip2-0.4.4
   rust-bzip2-sys-0.1.11+1.0.8
   rust-cast-0.3.0
   rust-cbc-0.1.2
   rust-cc-1.0.83
   rust-cfg-if-1.0.0
   rust-checked-int-cast-1.0.0
   rust-chrono-0.4.33
   rust-ciborium-0.2.2
   rust-ciborium-io-0.2.2
   rust-ciborium-ll-0.2.2
   rust-cipher-0.4.4
   rust-clap-2.34.0
   rust-clap-3.2.25
   rust-clap-4.5.48
   rust-clap-builder-4.5.48
   rust-clap-derive-3.2.25
   rust-clap-lex-0.2.4
   rust-clap-lex-0.7.5
   rust-color-quant-1.1.0
   rust-codespan-reporting-0.11.1
   rust-compiler-builtins-0.1.108
   rust-console-error-panic-hook-0.1.7
   rust-constant-time-eq-0.3.0
   rust-const-oid-0.7.1
   rust-const-oid-0.9.6
   rust-convert-case-0.4.0
   rust-core-foundation-sys-0.8.6
   rust-cpufeatures-0.2.17
   rust-crc-1.8.1
   rust-crc-3.2.1
   rust-crc32fast-1.4.2
   rust-crc-catalog-2.4.0
   rust-criterion-0.3.6
   rust-criterion-0.5.1
   rust-criterion-plot-0.4.5
   rust-criterion-plot-0.5.0
   rust-critical-section-1.2.0
   rust-crossbeam-0.8.4
   rust-crossbeam-channel-0.5.11
   rust-crossbeam-deque-0.8.5
   rust-crossbeam-epoch-0.9.18
   rust-crossbeam-queue-0.3.11
   rust-crossbeam-utils-0.8.20
   rust-crunchy-0.2.2
   rust-crypto-bigint-0.4.9
   rust-crypto-common-0.1.6
   rust-csv-1.3.0
   rust-csv-core-0.1.11
   rust-ctaphid-0.1.1
   rust-ctr-0.9.2
   rust-curve25519-dalek-derive-0.1.1
   rust-darling-0.13.4
   rust-darling-0.20.5
   rust-darling-core-0.13.4
   rust-darling-core-0.20.5
   rust-darling-macro-0.13.4
   rust-darling-macro-0.20.5
   rust-data-encoding-2.5.0
   rust-deflate64-0.1.9
   rust-defmt-0.3.6
   rust-defmt-macros-0.3.7
   rust-defmt-parser-0.3.4
   rust-der-0.5.1
   rust-der-0.6.1
   rust-der-0.7.8
   rust-deranged-0.3.11
   rust-der-derive-0.7.2
   rust-derive-arbitrary-1.3.2
   rust-der-parser-8.2.0
   rust-diff-0.1.13
   rust-digest-0.10.7
   rust-digest-0.9.0
   rust-dirs-next-2.0.0
   rust-dirs-sys-next-0.1.2
   rust-displaydoc-0.2.5
   rust-dlib-0.5.2
   rust-downcast-rs-1.2.0
   rust-ecdsa-0.14.8
   rust-ed25519-1.5.3
   rust-ed25519-2.2.3
   rust-ed25519-compact-1.0.16
   rust-ed25519-dalek-2.1.0
   rust-either-1.9.0
   rust-elliptic-curve-0.12.3
   rust-embedded-graphics-0.8.1
   rust-embedded-graphics-core-0.4.0
   rust-embedded-hal-0.2.7
   rust-embedded-hal-1.0.0
   rust-embedded-time-0.12.1
   rust-ena-0.14.2
   rust-enum-dispatch-0.3.12
   rust-enum-iterator-0.6.0
   rust-enum-iterator-derive-0.6.0
   rust-enumset-1.1.3
   rust-enumset-derive-0.8.1
   rust-env-logger-0.7.1
   rust-env-logger-0.9.3
   rust-env-logger-0.10.2
   rust-equivalent-1.0.1
   rust-errno-0.3.8
   rust-eyre-0.6.12
   rust-fastrand-2.0.1
   rust-fdeflate-0.3.4
   rust-ff-0.12.1
   rust-ff-0.13.1
   rust-fiat-crypto-0.1.20
   rust-fiat-crypto-0.2.7
   rust-fiat-crypto-0.3.0
   rust-filetime-0.2.23
   rust-fixedbitset-0.4.2
   rust-flate2-1.0.31
   rust-float-cmp-0.9.0
   rust-fnv-1.0.7
   rust-foldhash-0.1.3
   rust-form-urlencoded-1.2.1
   rust-frunk-0.4.2
   rust-frunk-core-0.4.2
   rust-frunk-derives-0.4.2
   rust-frunk-proc-macro-helpers-0.1.2
   rust-fugit-0.3.7
   rust-funty-2.0.0
   rust-futures-0.3.30
   rust-futures-channel-0.3.30
   rust-futures-core-0.3.30
   rust-futures-executor-0.3.30
   rust-futures-io-0.3.30
   rust-futures-macro-0.3.30
   rust-futures-sink-0.3.30
   rust-futures-task-0.3.30
   rust-futures-util-0.3.30
   rust-g2gen-1.1.0
   rust-g2p-1.1.0
   rust-g2poly-1.1.0
   rust-gcd-2.3.0
   rust-gdbstub-0.6.6
   rust-gdbstub-arch-0.2.4
   rust-generic-array-0.14.7
   rust-getrandom-0.1.16
   rust-ghostfat-0.5.0
   rust-glob-0.3.0
   rust-glob-0.3.1
   rust-group-0.12.1
   rust-group-0.13.0
   rust-half-1.8.2
   rust-half-2.6.0
   rust-hash32-0.2.1
   rust-hash32-0.3.1
   rust-hashbrown-0.12.3
   rust-hashbrown-0.14.3
   rust-hashbrown-0.15.1
   rust-heapless-0.7.17
   rust-heapless-0.8.0
   rust-heck-0.4.1
   rust-hermit-abi-0.1.19
   rust-hermit-abi-0.3.5
   rust-hex-0.3.2
   rust-hex-0.4.3
   rust-hex-literal-0.3.4
   rust-hex-literal-0.4.1
   rust-hex-literal-1.0.0
   rust-hidapi-1.5.0
   rust-hkdf-0.12.4
   rust-hmac-0.12.1
   rust-home-0.5.9
   rust-hoot-0.1.3
   rust-hootbin-0.1.1
   rust-http-0.2.11
   rust-httparse-1.8.0
   rust-humantime-1.3.0
   rust-humantime-2.2.0
   rust-iana-time-zone-0.1.60
   rust-iana-time-zone-haiku-0.1.2
   rust-ident-case-1.0.1
   rust-idna-0.3.0
   rust-image-0.25.5
   rust-indenter-0.3.3
   rust-indexmap-1.9.3
   rust-indexmap-2.2.2
   rust-inout-0.1.3
   rust-instant-0.1.12
   rust-is-terminal-0.4.10
   rust-itertools-0.10.5
   rust-itm-logger-0.1.2
   rust-itoa-1.0.1  ; for locales
   rust-itoa-1.0.10
   rust-jobserver-0.1.28
   rust-js-sys-0.3.68
   rust-keccak-0.1.5
   rust-lalrpop-0.19.12
   rust-lalrpop-util-0.19.12
   rust-lazy-static-1.4.0
   rust-libc-0.2.174
   rust-libloading-0.8.1
   rust-libm-0.1.4
   rust-libm-0.2.8
   rust-libredox-0.0.1
   rust-libredox-0.0.2
   rust-linked-list-allocator-0.10.5
   rust-linux-raw-sys-0.4.13
   rust-lock-api-0.4.11
   rust-lockfree-object-pool-0.1.6
   rust-log-0.4.22
   rust-lru-0.12.5
   rust-lzma-rs-0.3.0
   rust-managed-0.8.0
   rust-memchr-2.4.1  ; for locales
   rust-memchr-2.7.4
   rust-memoffset-0.6.5
   rust-merlin-2.0.1
   rust-merlin-3.0.0
   rust-micromath-2.1.0
   rust-minifb-0.26.0
   rust-minimal-lexical-0.2.1
   rust-miniz-oxide-0.4.4
   rust-miniz-oxide-0.7.2
   rust-munge-0.4.1
   rust-munge-macro-0.4.1
   rust-nalgebra-0.33.2
   rust-nb-0.1.3
   rust-nb-1.1.0
   rust-new-debug-unreachable-1.0.4
   rust-nix-0.24.3
   rust-nom-7.1.3
   rust-no-std-net-0.6.0
   rust-nu-ansi-term-0.46.0
   rust-num-0.3.1
   rust-num-bigint-0.4.4
   rust-num-complex-0.3.1
   rust-num-complex-0.4.6
   rust-num-conv-0.1.0
   rust-num-cpus-1.16.0
   rust-num-derive-0.3.3
   rust-num-derive-0.4.2
   rust-num-enum-0.5.11
   rust-num-enum-0.6.1
   rust-num-enum-derive-0.5.11
   rust-num-enum-derive-0.6.1
   rust-num-integer-0.1.46
   rust-num-iter-0.1.44
   rust-num-rational-0.3.2
   rust-num-rational-0.4.2
   rust-num-traits-0.2.18
   rust-oid-registry-0.6.1
   rust-once-cell-1.19.0
   rust-oorandom-11.1.3
   rust-opaque-debug-0.3.0
   rust-option-block-0.3.0
   rust-orbclient-0.3.47
   rust-os-str-bytes-6.6.1
   rust-overload-0.1.1
   rust-p256-0.11.1
   rust-packed-simd-2-0.3.8
   rust-packed-struct-0.10.1
   rust-packed-struct-codegen-0.10.1
   rust-packing-0.1.0
   rust-packing-0.2.0
   rust-packing-codegen-0.1.0
   rust-parking-lot-0.12.1
   rust-parking-lot-core-0.9.9
   rust-passwords-3.1.16
   rust-paste-1.0.15
   rust-pbkdf2-0.12.2
   rust-pem-0.8.3
   rust-pem-rfc7468-0.3.1
   rust-pem-rfc7468-0.7.0
   rust-percent-encoding-2.3.1
   rust-petgraph-0.6.4
   rust-phf-shared-0.10.0
   rust-pin-project-lite-0.2.13
   rust-pin-utils-0.1.0
   rust-pio-0.2.1
   rust-pio-parser-0.2.2
   rust-pio-proc-0.2.2
   rust-pkcs8-0.10.2
   rust-pkcs8-0.8.0
   rust-pkcs8-0.9.0
   rust-pkg-config-0.3.29
   rust-plotters-0.3.5
   rust-plotters-backend-0.3.5
   rust-plotters-svg-0.3.5
   rust-png-0.17.11
   rust-png-decoder-0.1.1
   rust-polyval-0.6.1
   rust-powerfmt-0.2.0
   rust-ppv-lite86-0.2.17
   rust-precomputed-hash-0.1.1
   rust-proc-macro2-1.0.36  ; for locales
   rust-proc-macro2-1.0.86
   rust-proc-macro-error-1.0.4
   rust-proc-macro-error-attr-1.0.4
   rust-proc-macro-hack-0.5.20+deprecated
   rust-protobuf-3.7.2
   rust-protobuf-codegen-3.7.2
   rust-protobuf-parse-3.7.2
   rust-protobuf-support-3.7.2
   rust-ptr-meta-0.1.4
   rust-ptr-meta-0.3.0
   rust-ptr-meta-derive-0.1.4
   rust-ptr-meta-derive-0.3.0
   rust-qrcode-0.12.0
   rust-quick-error-1.2.3
   rust-quick-xml-0.28.2
   rust-quote-1.0.15  ; for locales
   rust-quote-1.0.35
   rust-r0-1.0.0
   rust-radium-0.7.0
   rust-rancor-0.1.0
   rust-rand-0.6.5
   rust-rand-0.7.3
   rust-rand-0.8.5
   rust-rand-chacha-0.1.1
   rust-rand-chacha-0.2.2
   rust-rand-chacha-0.3.1
   rust-rand-core-0.3.1
   rust-rand-core-0.4.2
   rust-rand-core-0.5.1
   rust-rand-core-0.6.4
   rust-rand-hc-0.1.0
   rust-rand-hc-0.2.0
   rust-rand-isaac-0.1.1
   rust-rand-jitter-0.1.4
   rust-rand-os-0.1.3
   rust-rand-pcg-0.1.2
   rust-rand-xorshift-0.1.1
   rust-random-number-0.1.8
   rust-random-number-macro-impl-0.1.7
   rust-random-pick-1.2.16
   rust-rand-xorshift-0.3.0
   rust-rdrand-0.4.0
   rust-raw-window-handle-0.6.1
   rust-rayon-1.8.1
   rust-rayon-core-1.12.1
   rust-redox-syscall-0.4.1
   rust-redox-users-0.4.4
   rust-regex-1.6.0  ; for locales
   rust-regex-1.10.3
   rust-regex-automata-0.4.5
   rust-regex-syntax-0.6.27  ; for locales
   rust-regex-syntax-0.6.29
   rust-regex-syntax-0.8.2
   rust-rend-0.5.1
   rust-rfc6979-0.2.0
   rust-rfc6979-0.3.1
   rust-riscv-0.14.0
   rust-riscv-macros-0.2.0
   rust-riscv-pac-0.2.0
   rust-rkyv-0.4.3
   rust-rkyv-0.8.8
   rust-rkyv-derive-0.4.0
   rust-rkyv-derive-0.8.8
   rust-rustc-std-workspace-alloc-1.0.0
   rust-rustc-std-workspace-core-1.0.0
   rust-rustc-version-0.2.3
   rust-rustc-version-0.4.0
   rust-rust-fuzzy-search-0.1.1
   rust-rusticata-macros-4.1.0
   rust-rustix-0.38.31
   rust-rustls-0.22.2
   rust-rustls-pki-types-1.2.0
   rust-rustls-webpki-0.102.1
   rust-rustversion-1.0.14
   rust-rusb-0.8.1
   rust-rusb-0.9.4
   rust-libusb1-sys-0.6.4
   rust-libusb1-sys-0.7.0
   rust-libusb1-sys-0.5.0
   rust-ryu-1.0.9  ; for locales
   rust-ryu-1.0.16
   rust-same-file-1.0.6
   rust-scoped-tls-1.0.1
   rust-scopeguard-1.2.0
   rust-sct-0.7.1
   rust-sdl2-0.35.2
   rust-sdl2-sys-0.35.2
   rust-sec1-0.3.0
   rust-semver-0.9.0
   rust-semver-1.0.21
   rust-semver-parser-0.7.0
   rust-serde-1.0.135  ; for locales
   rust-serde-1.0.215
   rust-serde-bytes-0.11.14
   rust-serde-cbor-0.11.2
   rust-serde-derive-1.0.215
   rust-serde-json-1.0.78  ; for locales
   rust-serde-json-1.0.113
   rust-serde-repr-0.1.20
   rust-serde-spanned-0.6.9
   rust-serde-with-1.14.0
   rust-serde-with-macros-1.5.2
   rust-sha1-0.10.6
   rust-sha3-0.10.8
   rust-sharded-slab-0.1.7
   rust-signature-1.6.4
   rust-signature-2.2.0
   rust-simba-0.9.0
   rust-simd-adler32-0.3.7
   rust-siphasher-0.3.11
   rust-slab-0.4.9
   rust-smallvec-1.13.1
   rust-smoltcp-0.11.0
   rust-sntpc-0.3.7
   rust-spin-0.9.8
   rust-spinning-top-0.2.5
   rust-spki-0.5.4
   rust-spki-0.6.0
   rust-spki-0.7.3
   rust-stable-deref-trait-1.2.0
   rust-stats-alloc-0.1.10
   rust-string-cache-0.8.7
   rust-strsim-0.10.0
   rust-strsim-0.8.0
   rust-subtle-2.6.1
   rust-svd2utra-0.1.15
   rust-syn-1.0.109
   rust-syn-2.0.87
   rust-synstructure-0.12.6
   rust-tap-1.0.1
   rust-tempfile-3.10.0
   rust-term-0.7.0
   rust-termcolor-1.4.1
   rust-textwrap-0.11.0
   rust-textwrap-0.16.2
   rust-thiserror-1.0.63
   rust-thiserror-impl-1.0.63
   rust-thread-local-1.1.7
   rust-threadpool-1.8.1
   rust-time-0.3.36
   rust-time-core-0.1.2
   rust-time-macros-0.2.18
   rust-tiny-keccak-2.0.2
   rust-tinytemplate-1.2.1
   rust-tinyvec-1.6.0
   rust-tinyvec-macros-0.1.1
   rust-tock-registers-0.8.1
   rust-toml-0.5.11
   rust-toml-0.7.8
   rust-toml-datetime-0.6.11
   rust-toml-edit-0.19.15
   rust-tracing-0.1.40
   rust-tracing-attributes-0.1.27
   rust-tracing-core-0.1.32
   rust-tracing-log-0.2.0
   rust-tracing-subscriber-0.3.18
   rust-tracking-allocator-0.3.0
   rust-tungstenite-0.20.1
   rust-typenum-1.17.0
   rust-uf2-block-0.1.0
   rust-unicode-bidi-0.3.15
   rust-unicode-ident-1.0.12
   rust-unicode-normalization-0.1.22
   rust-unicode-width-0.1.11
   rust-unicode-xid-0.2.2  ; for locales
   rust-unicode-xid-0.2.4
   rust-universal-hash-0.5.1
   rust-untrusted-0.7.1
   rust-untrusted-0.9.0
   rust-ureq-2.9.5
   rust-url-2.3.1
   rust-usbd-bulk-only-transport-0.1.0
   rust-usbd-mass-storage-0.1.0
   rust-usbd-scsi-0.1.0
   rust-usbd-serial-0.1.1
   rust-utf-8-0.7.6
   rust-uuid-1.10.0
   rust-valuable-0.1.0
   rust-vcell-0.1.3
   rust-vec-map-0.8.2
   rust-version-check-0.9.4
   rust-version-compare-0.1.1
   rust-virtue-0.0.13
   rust-void-1.0.2
   rust-walkdir-2.4.0
   rust-wasi-0.11.0+wasi-snapshot-preview1
   rust-wasi-0.9.0+wasi-snapshot-preview1
   rust-wasm-bindgen-0.2.91
   rust-wasm-bindgen-backend-0.2.91
   rust-wasm-bindgen-futures-0.4.41
   rust-wasm-bindgen-macro-0.2.91
   rust-wasm-bindgen-macro-support-0.2.91
   rust-wasm-bindgen-shared-0.2.91
   rust-wasm-bindgen-test-0.3.41
   rust-wasm-bindgen-test-macro-0.3.41
   rust-wayland-client-0.29.5
   rust-wayland-commons-0.29.5
   rust-wayland-cursor-0.29.5
   rust-wayland-protocols-0.29.5
   rust-wayland-scanner-0.29.5
   rust-wayland-sys-0.29.5
   rust-webpki-roots-0.26.0
   rust-web-sys-0.3.68
   rust-which-4.4.2
   rust-winapi-0.3.9
   rust-winapi-i686-pc-windows-gnu-0.4.0
   rust-winapi-util-0.1.6
   rust-winapi-x86-64-pc-windows-gnu-0.4.0
   rust-windows-aarch64-gnullvm-0.48.5
   rust-windows-aarch64-gnullvm-0.52.0
   rust-windows-aarch64-msvc-0.48.5
   rust-windows-aarch64-msvc-0.52.0
   rust-windows-core-0.52.0
   rust-windows-i686-gnu-0.48.5
   rust-windows-i686-gnu-0.52.0
   rust-windows-i686-msvc-0.48.5
   rust-windows-i686-msvc-0.52.0
   rust-windows-sys-0.48.0
   rust-windows-sys-0.52.0
   rust-windows-targets-0.48.5
   rust-windows-targets-0.52.0
   rust-windows-x86-64-gnu-0.48.5
   rust-windows-x86-64-gnu-0.52.0
   rust-windows-x86-64-gnullvm-0.48.5
   rust-windows-x86-64-gnullvm-0.52.0
   rust-windows-x86-64-msvc-0.48.5
   rust-windows-x86-64-msvc-0.52.0
   rust-winnow-0.5.40
   rust-wyz-0.5.1
   rust-x11-dl-2.21.0
   rust-x25519-dalek-2.0.1
   rust-x509-parser-0.15.1
   rust-xcursor-0.3.5
   rust-xmas-elf-0.9.1
   rust-xml-rs-0.8.19
   rust-xous-0.9.69
   rust-xous-api-log-0.1.68
   rust-xous-api-names-0.9.70
   rust-xous-api-susres-0.9.68
   rust-xous-api-ticktimer-0.9.69
   rust-xous-ipc-0.10.9
   rust-xous-ipc-0.9.63
   rust-xous-riscv-0.5.6
   rust-xous-semver-0.1.5
   rust-xous-tts-backend-0.1.6
   rust-zero-0.1.3
   rust-zeroize-1.8.1
   rust-zeroize-derive-1.4.2
   rust-zip-2.1.6
   rust-zopfli-0.8.1
   rust-zstd-0.13.2
   rust-zstd-safe-7.2.1
   rust-zstd-sys-2.0.13+zstd.1.5.6
))

;;;
;;; Cargo inputs lookup (legacy, for future expansion)
;;;

(define-cargo-inputs lookup-cargo-inputs
  (bao1x-boot0 => bao1x-boot0-crate-inputs))
