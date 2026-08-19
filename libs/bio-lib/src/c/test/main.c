#include <stdint.h>

static inline __attribute__((always_inline)) volatile uint32_t pop_fifo0() {
    uint32_t rx;
    __asm__ volatile (
        "mv %0, x16"
        : "=r"(rx)   // output
    );
    return rx;
}

static inline __attribute__((always_inline)) volatile uint32_t pop_fifo1() {
    uint32_t rx;
    __asm__ volatile (
        "mv %0, x17"
        : "=r"(rx)   // output
    );
    return rx;
}

static inline __attribute__((always_inline)) void push_fifo0(uint32_t tx) {
    __asm__ volatile (
        "mv x16, %0"
        : // nil output
        : "r"(tx)
    );
}

__attribute__((naked, section(".text._start")))
void _start(void) {
    __asm__ volatile (
        "li sp, 0xe00\n"   // Load stack pointer to 0xE00
        "j main\n"              // Jump to main
    );
}

void main(void) {
    uint32_t a;
    uint32_t b;
    uint32_t i;
    uint32_t c = 0;

    while (1) {
        a = pop_fifo0();
        for(i = 0; i < a; i++) {
            b = pop_fifo1();
            c = b * a + c;
        }
        push_fifo0(c);
    }
}
