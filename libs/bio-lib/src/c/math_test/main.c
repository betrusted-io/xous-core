#include <stdint.h>
#include "fp_q12.h"
#include "bio.h"

void main(void) {
    int32_t in;
    fp_t arg;

    // computes fifo1 <- cosine(fifo0) + 1.0, where the value presented is in degrees as an FP value
    while (1) {
        in = pop_fifo0();

        // arg = arg * pi / 180
        arg = fp_mul(in, FP_PI);
        arg = fp_div(arg, FP_FROM_INT(180));
        arg = fp_add(fp_cos(arg), FP_FROM_INT(1));

        push_fifo1(arg);
    }
}
