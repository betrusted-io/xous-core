/* SP 800-230 KAT generator.
 * Emits a deterministic (slh_keygen_internal + slh_sign_internal) test vector
 * for whichever parameter set this file is compiled against.
 *
 * Inputs are fully explicit so the vector is reproducible independent of any
 * convention:
 *   sk_seed  = SPX_N bytes, value 0x00,0x01,0x02,...           (incrementing)
 *   sk_prf   = SPX_N bytes, value 0x40,0x41,...
 *   pk_seed  = SPX_N bytes, value 0x80,0x81,...
 *   opt_rand = SPX_N bytes, value 0xC0,0xC1,...   (the FIPS-205 addrnd / R input)
 *   message  = 33 ASCII bytes "NIST SP 800-230 SLH-DSA vector\n\0" minus NUL => 31 bytes
 * Context is empty (matches slh_sign_internal with no pre-hash / no context).
 */
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include "api.h"
#include "params.h"
#include "context.h"

/* Signature fingerprint hash. sha2.c cannot be linked into a SHAKE build (it
 * uses the SHA2-specific spx_ctx.state_seeded field), so pick the hash from the
 * source set actually linked: pass -DKAT_SHAKE for SHAKE parameter sets. The
 * fingerprint is sha256 for SHA2 sets and shake256 for SHAKE sets; the full
 * signature is always emitted to stderr regardless. */
#if defined(KAT_SHAKE)
#include "fips202.h"
static void kat_digest(unsigned char out[32], const unsigned char *in, size_t n) {
    shake256(out, 32, in, n);
}
#define KAT_DIGEST_NAME "shake256"
#else
#include "sha2.h"
static void kat_digest(unsigned char out[32], const unsigned char *in, size_t n) {
    sha256(out, in, n);
}
#define KAT_DIGEST_NAME "sha256"
#endif

static void put_hex(FILE *f, const char *label, const unsigned char *b, size_t n) {
    fprintf(f, "%s", label);
    for (size_t i = 0; i < n; i++) fprintf(f, "%02x", b[i]);
    fprintf(f, "\n");
}

int main(void) {
    unsigned char seed[CRYPTO_SEEDBYTES];      /* sk_seed || sk_prf || pk_seed */
    unsigned char optrand[SPX_N];
    unsigned char pk[CRYPTO_PUBLICKEYBYTES];
    unsigned char sk[CRYPTO_SECRETKEYBYTES];
    unsigned char sig[CRYPTO_BYTES];
    size_t siglen = 0;
    unsigned char sigdigest[32];

    const char *msg_s = "NIST SP 800-230 SLH-DSA vector\n";
    size_t mlen = strlen(msg_s);

    for (int i = 0; i < SPX_N; i++) {
        seed[i]          = (unsigned char)(0x00 + i);  /* sk_seed */
        seed[SPX_N + i]  = (unsigned char)(0x40 + i);  /* sk_prf  */
        seed[2*SPX_N + i]= (unsigned char)(0x80 + i);  /* pk_seed */
        optrand[i]       = (unsigned char)(0xC0 + i);
    }

    if (crypto_sign_seed_keypair(pk, sk, seed) != 0) { fprintf(stderr, "keygen failed\n"); return 1; }

    if (crypto_sign_signature_internal(sig, &siglen, (const unsigned char*)msg_s, mlen,
                                       NULL, 0, sk, optrand) != 0) {
        fprintf(stderr, "sign failed\n"); return 1;
    }

    /* Self-check: internal verify must accept. */
    if (crypto_sign_verify_internal(sig, siglen, (const unsigned char*)msg_s, mlen,
                                    NULL, 0, pk) != 0) {
        fprintf(stderr, "SELF-VERIFY FAILED\n"); return 2;
    }

    kat_digest(sigdigest, sig, siglen);

    printf("param_set        = %s\n", xstr(PARAMS));
    printf("n                = %d\n", SPX_N);
    printf("pk_bytes         = %d\n", CRYPTO_PUBLICKEYBYTES);
    printf("sig_bytes        = %zu\n", siglen);
    put_hex(stdout, "sk_seed          = ", seed, SPX_N);
    put_hex(stdout, "sk_prf           = ", seed + SPX_N, SPX_N);
    put_hex(stdout, "pk_seed          = ", seed + 2*SPX_N, SPX_N);
    put_hex(stdout, "opt_rand         = ", optrand, SPX_N);
    printf("message_ascii    = \"NIST SP 800-230 SLH-DSA vector\\n\"\n");
    put_hex(stdout, "message_hex      = ", (const unsigned char*)msg_s, mlen);
    put_hex(stdout, "pk               = ", pk, CRYPTO_PUBLICKEYBYTES);
    put_hex(stdout, "randomizer_R     = ", sig, SPX_N);
    put_hex(stdout, "sig_digest       = ", sigdigest, 32);
    printf("sig_digest_alg   = %s\n", KAT_DIGEST_NAME);
    /* Full signature to stderr so it can be redirected separately from the summary. */
    put_hex(stderr, "", sig, siglen);
    return 0;
}
