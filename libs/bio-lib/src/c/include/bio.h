#include <stdint.h>

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

__attribute__((naked, section(".text._start")))
void _start(void) {
    __asm__ volatile (
        "li sp, 0x1000\n"   // Load stack pointer to 0xE00
        "j main\n"              // Jump to main
    );
}