#!/usr/bin/env bash
#
# Reproducibly generate SLH-DSA (FIPS-205 + NIST SP 800-230) test vectors from
# the sphincs/sphincsplus C reference implementation, INDEPENDENTLY of the Rust
# crate under test.
#
# It clones the reference, checks out the FIPS-205 branch (big-endian FORS
# indexing, matching FIPS-205 and this crate), drops in the six SP 800-230
# parameter headers and the KAT driver, builds, and runs each vector.
#
# The three *-256-24 / *-192-24 / *-128-24 signers are SLOW (single hypertree
# of height 21-22 plus large FORS); each can take many minutes. Run detached.
#
# Requirements: git, gcc, make. No OpenSSL needed (the ref bundles sha2/fips202).

set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="${1:-/tmp/slh_vectors_build}"
REPO="https://github.com/sphincs/sphincsplus.git"
BRANCH="bas/fips205"   # FIPS-205 branch: big-endian FORS message_to_indices

echo "== cloning reference into $WORK =="
rm -rf "$WORK"
git clone --depth 1 --branch "$BRANCH" "$REPO" "$WORK" 2>/dev/null \
  || { echo "shallow branch clone failed; falling back to full clone"; \
       git clone "$REPO" "$WORK" && git -C "$WORK" checkout "$BRANCH"; }

REF="$WORK/ref"
echo "== installing SP 800-230 params + KAT driver into $REF =="
cp "$HERE"/params/params-sphincs-*-24.h "$REF/params/"
cp "$HERE"/slh_kat.c "$HERE"/slh_validate.c "$REF/"

cd "$REF"
CC=${CC:-gcc}
CFLAGS="-O3 -std=c99 -Wall -march=native"
COMMON="address.c randombytes.c merkle.c wots.c wotsx1.c utils.c utilsx1.c fors.c sign.c"
SHA2="sha2.c hash_sha2.c thash_sha2_simple.c"
SHAKE="fips202.c hash_shake.c thash_shake_simple.c"

build_one () {  # $1 = params name, $2 = hash source set, $3 = extra defines
  echo "  building kat_$1"
  # shellcheck disable=SC2086
  $CC $CFLAGS ${3:-} -DPARAMS=$1 slh_kat.c $COMMON $2 -o "$WORK/kat_$1"
}

# Optional: validate the oracle against the Rust crate's embedded primitive KATs
echo "== building + running primitive validation (sha2-128f) =="
$CC $CFLAGS -DPARAMS=sphincs-sha2-128f slh_validate.c address.c utils.c $SHA2 -o "$WORK/validate"
"$WORK/validate"

echo "== building all 6 SP 800-230 signers =="
for p in sphincs-sha2-128-24 sphincs-sha2-192-24 sphincs-sha2-256-24; do build_one "$p" "$SHA2"; done
for p in sphincs-shake-128-24 sphincs-shake-192-24 sphincs-shake-256-24; do build_one "$p" "$SHAKE" "-DKAT_SHAKE"; done
# Reference full set (end-to-end sanity against your Rust crate before -24):
build_one sphincs-sha2-128f "$SHA2"

echo
echo "== running signers (SLOW - detach the heavy ones if needed) =="
mkdir -p "$WORK/vectors"
for p in sphincs-sha2-128f \
         sphincs-sha2-128-24 sphincs-sha2-192-24 sphincs-sha2-256-24 \
         sphincs-shake-128-24 sphincs-shake-192-24 sphincs-shake-256-24; do
  echo "  running $p ..."
  "$WORK/kat_$p" 1>"$WORK/vectors/vec_$p.txt" 2>"$WORK/vectors/sig_$p.hex"
  grep -E 'param_set|sig_bytes|sig_sha256' "$WORK/vectors/vec_$p.txt"
done
echo "== done. vectors in $WORK/vectors =="
