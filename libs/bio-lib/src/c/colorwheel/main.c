#include <stdint.h>

#include "bio.h" // this must always be first
#include "fp_q16.h"
#include "ws2812.h"
#include "softdiv.h"

/* ------------------------------------------------------------------ */
/*  Configuration                                                     */
/* ------------------------------------------------------------------ */

#define NUM_LEDS        100   // maximum number of LEDs

/* ------------------------------------------------------------------ */
/*  Internals                                                         */
/* ------------------------------------------------------------------ */

typedef struct HsvColor {
  uint8_t h;
  uint8_t s;
  uint8_t v;
} HsvColor;

typedef struct RgbColor {
  uint8_t r;
  uint8_t g;
  uint8_t b;
} RgbColor;

// deliberately using global statics to test the case of statics in loading
static uint32_t led_buf[NUM_LEDS];
static HsvColor render_buf[NUM_LEDS];
static uint32_t hsv_state = 0;

RgbColor HsvToRgb(HsvColor hsv) {
    RgbColor rgb;
    unsigned char region, remainder, p, q, t;

    if (hsv.s == 0) {
        rgb.r = hsv.v;
        rgb.g = hsv.v;
        rgb.b = hsv.v;
        return rgb;
    }

    region = hsv.h / 43;
    remainder = (hsv.h - (region * 43)) * 6;

    p = (hsv.v * (255 - hsv.s)) >> 8;
    q = (hsv.v * (255 - ((hsv.s * remainder) >> 8))) >> 8;
    t = (hsv.v * (255 - ((hsv.s * (255 - remainder)) >> 8))) >> 8;

    switch (region) {
        case 0:
            rgb.r = hsv.v; rgb.g = t; rgb.b = p;
            break;
        case 1:
            rgb.r = q; rgb.g = hsv.v; rgb.b = p;
            break;
        case 2:
            rgb.r = p; rgb.g = hsv.v; rgb.b = t;
            break;
        case 3:
            rgb.r = p; rgb.g = q; rgb.b = hsv.v;
            break;
        case 4:
            rgb.r = t; rgb.g = p; rgb.b = hsv.v;
            break;
        default:
            rgb.r = hsv.v; rgb.g = p; rgb.b = q;
            break;
    }

    return rgb;
}

HsvColor RgbToHsv(RgbColor rgb) {
    HsvColor hsv;
    unsigned char rgbMin, rgbMax;

    rgbMin = rgb.r < rgb.g ? (rgb.r < rgb.b ? rgb.r : rgb.b) : (rgb.g < rgb.b ? rgb.g : rgb.b);
    rgbMax = rgb.r > rgb.g ? (rgb.r > rgb.b ? rgb.r : rgb.b) : (rgb.g > rgb.b ? rgb.g : rgb.b);

    hsv.v = rgbMax;
    if (hsv.v == 0) {
        hsv.h = 0;
        hsv.s = 0;
        return hsv;
    }

    hsv.s = (unsigned char) (255L * (((long) rgbMax - (long)rgbMin) / (long)hsv.v));
    if (hsv.s == 0) {
        hsv.h = 0;
        return hsv;
    }

    if (rgbMax == rgb.r)
        hsv.h = 0 + 43 * (rgb.g - rgb.b) / (rgbMax - rgbMin);
    else if (rgbMax == rgb.g)
        hsv.h = 85 + 43 * (rgb.b - rgb.r) / (rgbMax - rgbMin);
    else
        hsv.h = 171 + 43 * (rgb.r - rgb.g) / (rgbMax - rgbMin);

    return hsv;
}

void rainbow_update(uint32_t led_count, uint32_t rate) {
    HsvColor hsv;
    RgbColor rgb;

    if (led_count > NUM_LEDS) {
        led_count = NUM_LEDS;
    }

    uint16_t spacing = 256 / led_count;
    for (uint32_t i = 0; i < led_count; i++) {
        hsv.h = (uint16_t) ((hsv_state + spacing * i) % 256);
        hsv.s = 200;
        hsv.v = 64;
        render_buf[i] = hsv;
    }
    for (uint32_t i = 0; i < led_count; i++) {
        rgb = HsvToRgb(render_buf[i]);
        led_buf[i] = (((uint32_t) rgb.g << 16) & 0xFF0000) | (((uint32_t) rgb.r << 8) & 0xFF00) | (((uint32_t) rgb.b & 0xFF));
    }

    hsv_state = (hsv_state + rate) % 256;
}

void main(void) {
    uint32_t pin;
    uint32_t actual_leds;
    uint32_t rate;

    // blocks until these are configured
    pin = pop_fifo1();
    actual_leds = pop_fifo1();
    rate = pop_fifo1();

    while (1) {
        ws2812c(pin, led_buf, actual_leds);
        rainbow_update(actual_leds, rate);
        for (uint32_t i = 0; i < 100000; i++) {
            wait_quantum();
        }
    }
}
