#include <stdint.h>
#include <bio.h>

// pin is provided as the GPIO number to drive
// strip is an array of u32's that contain GRB data
// len is the length of the strip
void ws2812c(uint32_t pin, uint32_t *strip, uint32_t len) {
    uint32_t led;
    // sanity check the pin value
    if (pin > 31) {
        return;
    }
    uint32_t mask = 1 << pin;
    uint32_t antimask = ~mask;
    set_gpio_mask(mask);
    set_output_pins(mask);

    // ensure timing with a nil quantum here
    clear_gpio_pins_n(antimask);
    wait_quantum();
    // main loop
    for (uint32_t i = 0; i < len; i++) {
        led = strip[i];
        for (uint32_t bit = 0; bit < 24; bit++) {
            if ((led & 0x800000) == 0) {
                // 2 hi
                set_gpio_pins(mask);
                wait_quantum();
                wait_quantum();
                // 5 lo
                clear_gpio_pins_n(antimask);
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
            } else {
                // 5 hi
                set_gpio_pins(mask);
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                // 5 lo
                clear_gpio_pins_n(antimask);
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
            }
            led <<= 1;
        }
    }
}

static inline __attribute__((always_inline)) void nop() {
    __asm__ volatile ("c.nop" : : : "memory");
}

// A version of the loop that runs with a 48MHz BIO clock. The
// clock rate is slow enough that the instructions take longer
// than the quantum, so we have to do this by clock-counting.
void ws2812c_wfi(uint32_t pin, uint32_t *strip, uint32_t len) {
    uint32_t led;
    // sanity check the pin value
    if (pin > 31) {
        return;
    }
    uint32_t mask = 1 << pin;
    uint32_t antimask = ~mask;
    set_gpio_mask(mask);
    set_output_pins(mask);

    // ensure timing with a nil quantum here
    clear_gpio_pins_n(antimask);
    wait_quantum();
    // main loop
    for (uint32_t i = 0; i < len; i++) {
        led = strip[i];
        for (uint32_t bit = 0; bit < 24; bit++) {
            // note that between each LED pixel (every 25 bits) there is a little bit of an extra delay due
            // to the loop variable being updated & checked. This doesn't seem to impact the protocol integrity.
            if ((led & 0x800000) == 0) {
                // 2 hi
                set_gpio_pins(mask);
                // 5 lo
                clear_gpio_pins_n(antimask);
                nop();
                nop();
                nop();
                nop();
                nop();
            } else {
                // 5 hi
                set_gpio_pins(mask);
                nop();
                nop();
                nop();
                nop();
                nop();
                // 5 lo
                clear_gpio_pins_n(antimask);
                nop();
                nop();
                nop();
                nop();
                nop();
            }
            led <<= 1;
        }
    }
}