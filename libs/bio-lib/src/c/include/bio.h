#ifndef BIO_H
#define BIO_H

#include <stdint.h>

// ----------------------------------------------------------------
// entry point - this MUST be at the top of this file so that
// _start ends up at 0x0.
// ----------------------------------------------------------------

__attribute__((naked, section(".text._start")))
void _start(void) {
    __asm__ volatile (
        "li sp, 0x1000\n"   // Load stack pointer to 0x1000 - top of RAM
        "j main\n"              // Jump to main
    );
}

// pop_fifoN: read from reserved register xN.
// volatile prevents the compiler from caching or dropping the read.
static inline __attribute__((always_inline)) uint32_t pop_fifo0() {
    uint32_t rx;
    __asm__ volatile ("mv %0, x16" : "=r"(rx));
    return rx;
}

static inline __attribute__((always_inline)) uint32_t pop_fifo1() {
    uint32_t rx;
    __asm__ volatile ("mv %0, x17" : "=r"(rx));
    return rx;
}

static inline __attribute__((always_inline)) uint32_t pop_fifo2() {
    uint32_t rx;
    __asm__ volatile ("mv %0, x18" : "=r"(rx));
    return rx;
}

static inline __attribute__((always_inline)) uint32_t pop_fifo3() {
    uint32_t rx;
    __asm__ volatile ("mv %0, x19" : "=r"(rx));
    return rx;
}

// push_fifoN: write to reserved register xN.
// "memory" clobber is the key fix: it tells the compiler this asm has an
// observable side effect beyond the registers listed, so it cannot
// dead-code-eliminate the computation feeding the push even at -Os.
static inline __attribute__((always_inline)) void push_fifo0(uint32_t tx) {
    __asm__ volatile ("mv x16, %0" : : "r"(tx) : "memory");
}

static inline __attribute__((always_inline)) void push_fifo1(uint32_t tx) {
    __asm__ volatile ("mv x17, %0" : : "r"(tx) : "memory");
}

static inline __attribute__((always_inline)) void push_fifo2(uint32_t tx) {
    __asm__ volatile ("mv x18, %0" : : "r"(tx) : "memory");
}

static inline __attribute__((always_inline)) void push_fifo3(uint32_t tx) {
    __asm__ volatile ("mv x19, %0" : : "r"(tx) : "memory");
}

// GPIO accessors
static inline __attribute__((always_inline)) void set_gpio_mask(uint32_t mask) {
    __asm__ volatile ("mv x26, %0" : : "r"(mask) : "memory");
}
static inline __attribute__((always_inline)) uint32_t get_gpio_mask() {
    uint32_t mask;
    __asm__ volatile ("mv %0, x26" : "=r"(mask));
    return mask;
}

static inline __attribute__((always_inline)) void write_gpio_pins(uint32_t val) {
    __asm__ volatile ("mv x21, %0" : : "r"(val) : "memory");
}
static inline __attribute__((always_inline)) uint32_t read_gpio_pins() {
    uint32_t a;
    __asm__ volatile ("mv %0, x21" : "=r"(a));
    return a;
}

static inline __attribute__((always_inline)) void set_gpio_pins(uint32_t val) {
    __asm__ volatile ("mv x22, %0" : : "r"(val) : "memory");
}
// the `_n` name reminds us that *0* values clear the pin, and 1 does nothing
static inline __attribute__((always_inline)) void clear_gpio_pins_n(uint32_t val_n) {
    __asm__ volatile ("mv x23, %0" : : "r"(val_n) : "memory");
}

static inline __attribute__((always_inline)) void set_output_pins(uint32_t val) {
    __asm__ volatile ("mv x24, %0" : : "r"(val) : "memory");
}
static inline __attribute__((always_inline)) void set_input_pins(uint32_t val) {
    __asm__ volatile ("mv x25, %0" : : "r"(val) : "memory");
}

// Events
static inline __attribute__((always_inline)) void wait_quantum() {
    __asm__ volatile ("mv x20, zero" : : : "memory");
}
static inline __attribute__((always_inline)) uint32_t event_status() {
    uint32_t a;
    __asm__ volatile ("mv %0, x30" : "=r"(a));
    return a;
}
static inline __attribute__((always_inline)) void set_event_mask(uint32_t mask) {
    __asm__ volatile ("mv x27, %0" : : "r"(mask) : "memory");
}
static inline __attribute__((always_inline)) void set_event_bits(uint32_t mask) {
    __asm__ volatile ("mv x28, %0" : : "r"(mask) : "memory");
}
static inline __attribute__((always_inline)) void clear_event_bits(uint32_t mask) {
    __asm__ volatile ("mv x29, %0" : : "r"(mask) : "memory");
}

// Debug
static inline __attribute__((always_inline)) uint32_t core_id() {
    uint32_t a;
    __asm__ volatile ("mv %0, x31" : "=r"(a));
    return a >> 30;
}
static inline __attribute__((always_inline)) uint32_t aclk_counter() {
    uint32_t a;
    __asm__ volatile ("mv %0, x31" : "=r"(a));
    return a & 0x3FFFFFFF;
}
static inline __attribute__((always_inline)) uint32_t raw_x31() {
    uint32_t a;
    __asm__ volatile ("mv %0, x31" : "=r"(a));
    return a;
}

#endif