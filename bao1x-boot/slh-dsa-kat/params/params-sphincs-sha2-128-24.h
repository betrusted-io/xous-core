#ifndef SPX_PARAMS_H
#define SPX_PARAMS_H

/* NIST SP 800-230 (ipd) limited-signature parameter set: params-sphincs-sha2-128-24
 * 2^24 signature limit per signing key. d=1 (single-layer hypertree),
 * adjusted Winternitz parameter w=4. */

#define SPX_NAMESPACE(s) SPX_##s

#define SPX_N 16
#define SPX_FULL_HEIGHT 22
#define SPX_D 1
#define SPX_FORS_HEIGHT 24
#define SPX_FORS_TREES 6
#define SPX_WOTS_W 4
#define SPX_SHA512 0

#define SPX_ADDR_BYTES 32

/* WOTS parameters (SP 800-230 adjusted Winternitz). */
#define SPX_WOTS_LOGW 2
#define SPX_WOTS_LEN1 (8 * SPX_N / SPX_WOTS_LOGW)
/* SPX_WOTS_LEN2 = floor(log(len1*(w-1))/log(w)) + 1, precomputed for this set. */
#define SPX_WOTS_LEN2 4
#define SPX_WOTS_LEN (SPX_WOTS_LEN1 + SPX_WOTS_LEN2)
#define SPX_WOTS_BYTES (SPX_WOTS_LEN * SPX_N)
#define SPX_WOTS_PK_BYTES SPX_WOTS_BYTES

#define SPX_TREE_HEIGHT (SPX_FULL_HEIGHT / SPX_D)
#if SPX_TREE_HEIGHT * SPX_D != SPX_FULL_HEIGHT
    #error SPX_D should always divide SPX_FULL_HEIGHT
#endif

/* FORS parameters. */
#define SPX_FORS_MSG_BYTES ((SPX_FORS_HEIGHT * SPX_FORS_TREES + 7) / 8)
#define SPX_FORS_BYTES ((SPX_FORS_HEIGHT + 1) * SPX_FORS_TREES * SPX_N)
#define SPX_FORS_PK_BYTES SPX_N

/* Resulting SPX sizes. */
#define SPX_BYTES (SPX_N + SPX_FORS_BYTES + SPX_D * SPX_WOTS_BYTES +\
                   SPX_FULL_HEIGHT * SPX_N)
#define SPX_PK_BYTES (2 * SPX_N)
#define SPX_SK_BYTES (2 * SPX_N + SPX_PK_BYTES)

#include "../sha2_offsets.h"

#endif
