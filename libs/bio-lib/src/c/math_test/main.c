#include <stdint.h>
#include "fp.h"
#include "bio.h"

void main(void) {
    int32_t in;
    fp_t arg;

    // fp_t state = FP_FROM_INT(1);
    // fp_t a;
    // computes fifo1 <- cosine(fifo0), where the value presented is in degrees
    while (1) {
        in = pop_fifo0();

        // arg = arg * pi / 180
        arg = fp_mul(in, FP_PI);
        arg = fp_div(arg, FP_FROM_INT(180));
        arg = fp_add(fp_cos(arg), FP_FROM_INT(1));

        /*
        arg = FP_FROM_INT(in);
        a = fp_mul(arg, FP_FROM_INT(2));
        a = fp_add(state, a);
        result = fp_div(a, state);
        state = arg;
        */
        push_fifo1(arg);
    }
}
