/* Cross-check the C reference primitives against the RustCrypto slh-dsa
 * crate's embedded KAT values (hashes.rs), for sphincs-sha2-128f.
 *   prf_msg: sk_prf=[0;16], opt_rand=[1;16], msg=[2;32]
 *            expected 6a4b5cf23911d4f3a6591d7003445316
 *   h_msg:   R=[0;16], pk_seed=[1;16], pk_root=[2;16], msg=[3;32]
 *            expected 56658221f675d907a309255e8faef639d11e6a1118fa05d3bbd26179a7e0a54a7f5b
 */
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include "params.h"
#include "context.h"
#include "sha2.h"
#include "hash.h"

static int chk(const char *name, const unsigned char *got, size_t n, const char *exp_hex) {
    char h[256]; for (size_t i=0;i<n;i++) sprintf(h+2*i,"%02x",got[i]); h[2*n]=0;
    int ok = strcmp(h, exp_hex)==0;
    printf("%-10s got=%s\n%-10s exp=%s   [%s]\n", name, h, "", exp_hex, ok?"MATCH":"MISMATCH");
    return ok;
}

int main(void) {
    spx_ctx ctx; memset(&ctx, 0, sizeof ctx);
    int ok = 1;

    /* prf_msg via gen_message_random */
    unsigned char sk_prf[16], optrand[16], m1[32], R[SPX_N];
    memset(sk_prf,0,16); memset(optrand,1,16); memset(m1,2,32);
    gen_message_random(R, sk_prf, optrand, NULL, 0, m1, 32, &ctx);
    ok &= chk("prf_msg", R, 16, "6a4b5cf23911d4f3a6591d7003445316");

    /* h_msg replicated from FIPS-205 formula using reference sha256 + mgf1_256 */
    unsigned char Rr[16], pk_seed[16], pk_root[16], m2[32];
    memset(Rr,0,16); memset(pk_seed,1,16); memset(pk_root,2,16); memset(m2,3,32);
    unsigned char buf[16+16+16+32];
    memcpy(buf,Rr,16); memcpy(buf+16,pk_seed,16); memcpy(buf+32,pk_root,16); memcpy(buf+48,m2,32);
    unsigned char seedhash[32];
    sha256(seedhash, buf, sizeof buf);
    unsigned char mgfin[16+16+32];
    memcpy(mgfin,Rr,16); memcpy(mgfin+16,pk_seed,16); memcpy(mgfin+32,seedhash,32);
    unsigned char out[34];
    mgf1_256(out, 34, mgfin, sizeof mgfin);
    ok &= chk("h_msg", out, 34, "56658221f675d907a309255e8faef639d11e6a1118fa05d3bbd26179a7e0a54a7f5b");

    printf("\nOVERALL: %s\n", ok?"ALL MATCH - C oracle agrees with Rust crate KATs":"MISMATCH");
    return ok?0:1;
}
