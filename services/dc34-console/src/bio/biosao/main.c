#include <stdint.h>

#include "bio.h" // this must always be first

#define SAO_GPIO1 21
#define SAO_GPIO2 22
#define SAO_GPIO3 30
#define SAO_GPIO4 31

#define QUANTUM_PER_MS 1000 // assumes `--quantum 1MHz` passed as part of init
// gpio config assumes `--sao 1,3`

#define FIFO3_EMPTY_MASK FIFO_EVENT_MASK(3, 0)
#define FIFO3_AVAILABLE_MASK FIFO_EVENT_MASK(3, 1)

/* --- Tunables ---------------------------------------------------------- */
#define DEPTH               32
#define TOUCH_MARGIN        2
#define DEBOUNCE_COUNT      100
/* ----------------------------------------------------------------------- */

typedef struct {
    uint8_t  touched;
    uint8_t  glitch_count;
    uint32_t samples[DEPTH];
    int      samp_ptr;
    uint8_t  baseline_done;
    uint32_t baseline;
} TouchState;

void touch_init(TouchState *s)
{
    s->glitch_count     = DEBOUNCE_COUNT;
    s->touched          = 0;
    for (int i = 0; i < DEPTH; i++) {
        s->samples[i] = 0;
    }
    s->samp_ptr = 0;
    s->baseline_done = 0;
    s->baseline = 0;
}

int touch_update(TouchState *s, uint32_t sample, int rebaseline)
{
    /* Forced re-baseline: snap to current sample, schedule fast settle */
    if (rebaseline) {
        touch_init(s);
        return 0;
    }

    s->samples[s->samp_ptr] = sample;
    s->samp_ptr ++;

    uint32_t acc = 0;
    for (int i = 0; i < DEPTH; i++) {
        acc += s->samples[i];
    }
    uint32_t avg = acc / DEPTH;

    if ((s->samp_ptr == DEPTH) && (s->baseline_done == 0)) {
        s->baseline_done = 1;
        s->baseline = avg;
        s->glitch_count = DEBOUNCE_COUNT;
    }
    s->samp_ptr %= DEPTH;

    if (s->baseline_done != 0) {
        if (s->touched == 0) {
            if (avg > s->baseline * TOUCH_MARGIN) {
                s->glitch_count --;
                if (s->glitch_count == 0) {
                    s->touched = 1;
                    s->glitch_count = DEBOUNCE_COUNT;
                }
            } else {
                s->glitch_count = DEBOUNCE_COUNT;
            }
        } else {
            // touched == 1
            if (avg < s->baseline * TOUCH_MARGIN) {
                s->glitch_count --;
                if (s->glitch_count == 0) {
                    s->touched = 0;
                    s->glitch_count = DEBOUNCE_COUNT;
                }
            } else {
                s->glitch_count = DEBOUNCE_COUNT;
            }
        }
    }

    return s->touched | avg << 16;
}

void main(void) {
    uint32_t aclk_start;
    uint32_t aclk_end;
    uint32_t elapsed;
    int touched;
    TouchState s;

    // setup input and output pins
    uint32_t input_mask = 1 << SAO_GPIO3;
    uint32_t output_mask = 1 << SAO_GPIO1;
    set_gpio_mask(input_mask | output_mask);
    set_input_pins(input_mask);
    set_output_pins(output_mask);

    // setup touch state
    touch_init(&s);

    set_output_pins(input_mask | output_mask);
    clear_gpio_pins_n(!input_mask); // drives the pin low
    int first_time = 1;
    while (1) {
        for (uint32_t low_wait = 0; low_wait < QUANTUM_PER_MS; low_wait++) {
            wait_quantum();
        }

        aclk_start = aclk_counter();
        set_input_pins(input_mask);
        while ((read_gpio_pins() & input_mask) == 0) {
            // wait for rising edge
        }
        aclk_end = aclk_counter();

        if (aclk_end > aclk_start) {
            elapsed = aclk_end - aclk_start;
        } else {
            elapsed = (0x3fffffff - aclk_end) + aclk_start;
        }

        touched = touch_update(&s, elapsed, first_time);
        first_time = 0;

        if ((event_status() & FIFO3_EMPTY_MASK) != 0) {
            push_fifo3(touched);
        }

        set_output_pins(input_mask);
        clear_gpio_pins_n(!input_mask); // drives the pin low
        // sets output pin to 1 if touched
        if ((touched & 1) != 0) {
            set_gpio_pins(output_mask);
        } else {
            clear_gpio_pins_n(!output_mask);
        }
    }
}

#if 0
// reference code for attackers -
//
// this scans through all of memory and tries to extract any data it can.
// If the host firewall is active, it only return data from the BIO segment,
// which starts at 0x0

void main(void) {
    uint32_t counter = 0;
    uint32_t *raw_ptr = (uint32_t *) 0x10000000;
    uint32_t sample = 0;
    // search for data in all of memory!
    raw_ptr[0] = 0x0;
    while (1) {
        sample = raw_ptr[counter];
        counter++;
        if (sample != 0) {
            // this automatically blocks
            push_fifo3(counter * 4 + (uint32_t) raw_ptr);
            push_fifo3(raw_ptr[counter]);
        }
    }
}
#endif