// soft_div.c
// Software implementations of integer division/remainder routines
// for freestanding targets lacking hardware divide.
//
// Naming follows the GCC/LLVM compiler-rt ABI so the compiler's
// implicit calls resolve automatically.
//
// Note that there is no link-time optimization in this flow, so
// this file should only be included if a divider is required.

#include <stdint.h>

// ----------------------------------------------------------------
// Unsigned 32-bit divide/remainder (the foundation for everything)
// ----------------------------------------------------------------

// Returns quotient; optionally writes remainder to *rem_out.
static uint32_t udiv32_rem(uint32_t n, uint32_t d, uint32_t *rem_out) {
    // Division by zero: match hardware "undefined" behaviour by returning
    // all-ones quotient and the original numerator as remainder,
    // consistent with what real RISC-V hardware would do.
    if (d == 0) {
        if (rem_out) *rem_out = n;
        return 0xFFFFFFFFu;
    }

    uint32_t q = 0;
    uint32_t r = 0;

    for (int i = 31; i >= 0; i--) {
        r = (r << 1) | ((n >> i) & 1u);
        if (r >= d) {
            r -= d;
            q |= (1u << i);
        }
    }

    if (rem_out) *rem_out = r;
    return q;
}

// ----------------------------------------------------------------
// Compiler-rt ABI entry points
// ----------------------------------------------------------------

uint32_t __udivsi3(uint32_t n, uint32_t d) {
    return udiv32_rem(n, d, 0);
}

uint32_t __umodsi3(uint32_t n, uint32_t d) {
    uint32_t r;
    udiv32_rem(n, d, &r);
    return r;
}

int32_t __divsi3(int32_t n, int32_t d) {
    // Compute sign, then delegate to unsigned core.
    int negative = (n < 0) ^ (d < 0);
    uint32_t un = (n < 0) ? (uint32_t)(-(uint32_t)n) : (uint32_t)n;  // careful: avoids UB on INT_MIN
    uint32_t ud = (d < 0) ? (uint32_t)(-(uint32_t)d) : (uint32_t)d;
    uint32_t uq = udiv32_rem(un, ud, 0);
    return negative ? -(int32_t)uq : (int32_t)uq;
}

int32_t __modsi3(int32_t n, int32_t d) {
    // Remainder has the sign of the dividend (C11 §6.5.5).
    int negative = (n < 0);
    uint32_t un = (n < 0) ? (uint32_t)(-(uint32_t)n) : (uint32_t)n;
    uint32_t ud = (d < 0) ? (uint32_t)(-(uint32_t)d) : (uint32_t)d;
    uint32_t r;
    udiv32_rem(un, ud, &r);
    return negative ? -(int32_t)r : (int32_t)r;
}