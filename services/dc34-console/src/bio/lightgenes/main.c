#include <stdint.h>

#include "bio.h" // this must always be first
#include "fp_q16.h"
#include "ws2812.h"
#include "softdiv.h"

#define QUANTUM_NS     150    // assumed quantum interval in nanoseconds
#define QUANTUM_PER_MS 6670   // quantum to wait per millisecond delay

// Only define for UBER builds
// Don't forget to re-run `python3 -m ziglang build "-Dmodule=lightgenes"` inside src/bio
// after uncommenting this so that the C files are turned into .rs artifacts.
// #define UBER

/* ------------------------------------------------------------------ */
/*  Configuration                                                     */
/* ------------------------------------------------------------------ */

#define NUM_LEDS        10
#define LED_OFFSET      2 // for the "eyes"

/* ------------------------------------------------------------------ */
/*  Internals                                                         */
/* ------------------------------------------------------------------ */

typedef struct Color Color;
struct Color {
  uint8_t g;
  uint8_t r;
  uint8_t b;
};

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

typedef struct genome {
  uint8_t  cd_period;
  uint8_t  cd_rate;
  uint8_t  cd_dir;
  uint8_t  sat;
  uint8_t  hue_ratedir;
  uint8_t  hue_base;
  uint8_t  hue_bound;
  uint8_t  lin;
  uint8_t  nonlin;  // nonlinearity selector
} genome; // 9 bytes for a haploid gamete

// deliberately using global statics to test the case of statics in loading
static uint32_t led_buf[NUM_LEDS];
static uint32_t loop_state = 0;
static uint32_t time_ms = 0;
static genome diploid;
static uint32_t reftime_lg = 0;
#ifdef UBER
static uint8_t shift = 3;
#else
static uint8_t shift = 5;  // overall dimming for power savings
#endif
static uint8_t shift_eyes = 3; // less shift for the eyes as they are dimmer
static uint32_t jack_eyes = 0; // whether or not to flash the jack's eyes

static void ledSetRGB(uint32_t *led_buf, int i, uint8_t r, uint8_t g, uint8_t b, uint32_t shift) {
  led_buf[i] = ((((uint32_t) g << 16) >> shift) & 0xFF0000)
      | ((((uint32_t) r << 8) >> shift) & 0xFF00)
      | (((uint32_t) b & 0xFF)) >> shift;
}

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

/*
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
*/


// saturating subtract. returns a-b, stopping at 0. assumes 8-bit types
uint8_t satsub_8(uint8_t a, uint8_t b) {
  if (a >= b)
    return (a - b);
  else
    return 0;
}

// saturating add, returns a+b, stopping at 255. assumes 8-bit types
uint8_t satadd_8(uint8_t a, uint8_t b) {
  uint16_t c = (uint16_t) a + (uint16_t) b;

  if (c > 255)
    return (uint8_t) 255;
  else
    return (uint8_t) (c & 0xFF);
}

// saturating add, returns a+b, stopping at limit.
uint8_t satadd_8_limit(uint8_t a, uint8_t b, uint8_t limit) {
  uint16_t c = (uint16_t) a + (uint16_t) b;

  if (c > limit)
    return (uint8_t) limit;
  else
    return (uint8_t) (c & 0xFF);
}

// saturating subtract, acting on a whole RGB pixel
Color satsub_8p(Color c, uint8_t val) {
  Color rc;
  rc.r = satsub_8( c.r, val );
  rc.g = satsub_8( c.g, val );
  rc.b = satsub_8( c.b, val );

  return rc;
}

// saturating add, acting on a whole RGB pixel
Color satadd_8p( Color c, uint8_t val ) {
  Color rc;
  rc.r = satadd_8( c.r, val );
  rc.g = satadd_8( c.g, val );
  rc.b = satadd_8( c.b, val );

  return rc;
}

int16_t map_16(int16_t x, int16_t in_min, int16_t in_max, int16_t out_min, int16_t out_max)
{
  fp_t x16, in_min16, in_max16, out_min16, out_max16;
  fp_t result16;

  x16 = FP_FROM_INT(x);
  in_min16 = FP_FROM_INT(in_min);
  in_max16 = FP_FROM_INT(in_max);
  out_min16 = FP_FROM_INT(out_min);
  out_max16 = FP_FROM_INT(out_max);

  result16 = fp_div( fp_sub(out_max16, out_min16), fp_sub(in_max16, in_min16) );
  result16 = fp_mul( result16, fp_sub( x16, in_min16 ));
  result16 = fp_add( result16, out_min16 );
  return FP_TO_INT(result16);
}

int map(int x, int in_min, int in_max, int out_min, int out_max)
{
  return (x - in_min) * (out_max - out_min) / (in_max - in_min) + out_min;
}

static void do_lightgene(uint32_t actual_leds) {
  uint32_t *fb = led_buf;
  uint32_t count = actual_leds;
  uint32_t loop = loop_state & 0x1FF;
  uint32_t shoot;
  HsvColor hsvC;
  RgbColor rgbC;
  uint32_t i;
  uint32_t tau;
  uint32_t curtime, indextime;
  //float time, space;
  //float twopi;
  //float spacetime;
  fp_t time, space;
  fp_t spacetime;
  uint8_t overrideHSV = 0;
  uint32_t hue_rate;
  uint8_t hue_dir;
  uint32_t hue_temp;
  RgbColor eye_left = {0, 0, 0};
  RgbColor eye_right = {0, 0, 0};
  // diploid is static to this function and set when the lightgene is selected

  tau = (uint32_t) map(diploid.cd_rate, 0, 255, 60, 700);
  curtime = time_ms / 10;
  if( (curtime - reftime_lg) > tau )
    reftime_lg = curtime;
  indextime = reftime_lg - curtime;

  for( i = 0; i < count - LED_OFFSET; i++ ) {
    overrideHSV = 0;
    // compute one pixel's color
    // count is the current pixel index
    // loop is the current point in effect cycle, e.g. all effects loop on a 0-511 basis
    // hue chromosome
    hue_rate = (uint32_t) diploid.hue_ratedir & 0xF;
    hue_dir = (((diploid.hue_ratedir >> 4) & 0xF) > 10) ? 1 : 0;
    /*
      refactor: we want the pattern applied from 0-7 to be inversely applied from 8-15
      0 1 2 3 4 5 6 7  7 6 5 4 3 2 1 0
     */
    // 254L cheesily avoids rounding errors.
    if( !hue_dir ) {
      hue_temp = ((128 / ((count - LED_OFFSET) / 2)) * i + (loop * hue_rate)) - 0L;
      hue_temp &= 0x1FF;
      if( hue_temp <= 0xFF ) {
	    // hue_temp = hue_temp;
      } else {
	    hue_temp = (511-hue_temp);
      }
    } else {
      hue_temp = ((128 / ((count - LED_OFFSET) / 2)) * i - (loop * hue_rate)) - 0L;
      hue_temp &= 0x1FF;
      if( hue_temp <= 0xFF ) {
	    // hue_temp = hue_temp;
      } else {
	    hue_temp = (511-hue_temp);
      }
    }
    hsvC.h = (uint8_t) map_16( (int16_t) hue_temp, (int16_t) 0, (int16_t) 255,
		     (int16_t) diploid.hue_base, (int16_t) diploid.hue_bound );

    // saturation chromosome
    hsvC.s = diploid.sat;

    // compute the value overlay
    // use cos b/c value is 1.0 when input is 0
    // value = 255 * cos( cd_period * 2pi * (i/(count-1))
    //                    +/- (indextime / tau(cd_rate)) * 2pi )
    // sign of rate is determined by cd_dir

    //twopi = 3.14159265359 * 2.0;
    // space = (2pi * diploid.cd_period * i) / (count-1))
    space =   fp_mul(
                  FP_TWO_PI,
                  fp_mul(
                    FP_FROM_INT(diploid.cd_period),
                    fp_div(
                      FP_FROM_INT(i),
                      FP_FROM_INT(count - 1 - LED_OFFSET)
                    )
                  )
                );

	  //space = twopi * diploid.cd_period * ((float)i / ((float)count - 1.0));

    // time = 2pi * (indextime) / tau
    time = fp_div(fp_mul(FP_TWO_PI, FP_FROM_INT(indextime)), FP_FROM_INT(tau) );
    //time = twopi * (float) indextime / (float) tau;

    //    time = FP_FROM_INT(0);

    // space +/- time based on direction
    if( diploid.cd_dir > 128 ) {
      spacetime = fp_add( space, time );
      //spacetime = space + time;
    } else {
      spacetime = fp_sub( space, time );
      //spacetime = space - time;
    }

    // hsv.v = 127 * (1 + cos(spacetime))
    hsvC.v = (uint8_t) FP_TO_INT(
        fp_mul(FP_FROM_INT(127),
           fp_add(FP_FROM_INT(1), fp_cos(spacetime))));
    //hsvC.v = (uint8_t) 127.0 * (1.0 + cosf(spacetime));

    if( diploid.nonlin > 127 )
      // add some nonlinearity to tune down the total brightness - saves battery life while improving aesthetics
      hsvC.v = (uint8_t) (((uint32_t) hsvC.v * (uint32_t) hsvC.v) >> 8 & 0xFF);

    rgbC = HsvToRgb(hsvC);

    // now compute lin effect, but only if the threshold is met
    if( diploid.lin < 88 ) {  // rare variant after a summing expression ~3% chance
      shoot = (loop / 2) % (count - LED_OFFSET);
      if( shoot == i ) {
	       overrideHSV = 1;
      }

      if (shoot < ((count - LED_OFFSET) / 2)) {
        eye_left.r = 192;
        eye_left.g = 192;
        eye_left.b = 192;
        eye_right.r = 0;
        eye_right.g = 0;
        eye_right.b = 0;
      } else {
        eye_left.r = 0;
        eye_left.g = 0;
        eye_left.b = 0;
        eye_right.r = 192;
        eye_right.g = 192;
        eye_right.b = 192;
      }
    } else {
      if (i == 0) {
        eye_left = rgbC;
      }
      if (i == (count - LED_OFFSET) / 2) {
        eye_right = rgbC;
      }
    }

    // go from RGB to HSV for a particular pixel
    if( !overrideHSV ) {
      ledSetRGB(fb, i + LED_OFFSET, rgbC.r, rgbC.g, rgbC.b, shift);
    } else {
      ledSetRGB(fb, i + LED_OFFSET, 160, 160, 160, shift);
    }
  }

#ifdef UBER
    ledSetRGB(fb, 0, eye_left.r, eye_left.g, eye_left.b, shift_eyes);
    ledSetRGB(fb, 1, eye_right.r, eye_right.g, eye_right.b, shift_eyes);
#else
  // make sure the "eyes" are off in this mode to save power
  if (jack_eyes == 0) {
    ledSetRGB(fb, 0, 0, 0, 0, shift);
    ledSetRGB(fb, 1, 0, 0, 0, shift);
  } else {
    ledSetRGB(fb, 0, eye_left.r, eye_left.g, eye_left.b, shift_eyes);
    ledSetRGB(fb, 1, eye_right.r, eye_right.g, eye_right.b, shift_eyes);
  }
#endif
}

void test_pattern(uint32_t actual_leds) {
  uint32_t *fb = led_buf;
  uint32_t count = actual_leds;
  uint32_t loop = loop_state & 0x1FF;
  uint32_t shoot;
  uint32_t i;
  uint32_t light = 0;

  for(i = 0; i < count; i++) {
    light = 0;
    shoot = (loop / 2) % count;
    if( shoot == i ) {
        light = 1;
    }
    if( !light ) {
      ledSetRGB(fb, i, 0, 0, 0, shift);
    } else {
      if (i >= 2) {
        ledSetRGB(fb, i, 0x35, 0x35, 0x35, shift);
      } else {
        ledSetRGB(fb, i, 0x35, 0x35, 0x35, shift_eyes);
      }
    }
  }
}

void main(void) {
    uint32_t pin;
    uint32_t actual_leds;

    uint32_t index = 0;
    uint32_t incoming = 0;
    uint8_t *gene_ptr = (uint8_t *) &diploid;
    uint32_t init = 0;

    // this is used to skip LED updating during e.g. clock rate changes
    uint32_t led_gate = 0;
    uint32_t wfi_mode = 0;

    // set event mask to 0, so that we don't halt when the event register is read.
    // this allows us to poll the event register without blocking on it.
    set_event_mask(0);

    // FIFO1
    uint32_t fifo1_slot0_mask = FIFO_EVENT_MASK(1, 0);

    // blocks until these are configured
    pin = pop_fifo1();
    actual_leds = pop_fifo1();

    while (1) {
        while ((event_status() & fifo1_slot0_mask) != 0) {
            incoming = pop_fifo1();
            // modify gates
            if ((incoming & 0x80000000) != 0) {
              // this is the number of loop iters to wait before resuming
              // each loop is about 35ms
              led_gate = 3;
              continue;
            }
            if ((incoming & 0x20000000) != 0) {
              jack_eyes = 1;
              continue;
            }
            if ((incoming & 0x10000000) != 0) {
              jack_eyes = 0;
              continue;
            }
            if ((incoming & 0xFFFFFFFE) == 0x08000000) {
              wfi_mode = incoming & 0x1;
              // ack the wfi_mode setting by pushing "anything" into the FIFO
              push_fifo2(42);
              continue;
              if (wfi_mode == 0) {
                // add more time coming out of WFI mode to prevent bright-glitch on WFI exit
                // this has been padded by at least 2 beyond the minimum value to handle device variances
                led_gate = 8;
              }
            }

            // extract index & force it to be in-range
            if ((incoming & 0x40000000) != 0) {
              index = (incoming >> 8) & 0xff;
              index = index % sizeof(genome);
              gene_ptr[index] = (uint8_t) (incoming & 0xff);
              init = 1;
            }
        }

        if (led_gate == 0) {
          if (wfi_mode != 0) {
            ws2812c_wfi(pin, led_buf, actual_leds);
          } else {
            ws2812c(pin, led_buf, actual_leds);
          }
        }
        if (init == 0) {
          test_pattern(actual_leds);
        } else {
          do_lightgene(actual_leds);
        }
        loop_state += 1;

        // decompose this into an outer loop that counts # of milliseconds
        // and then an inner loop that counts milliseconds so we have a roughly
        // "real time" sense of time in time_ms (in reality runs a bit slow
        // because we don't trim out the bitbang & next compute times)
        for (uint32_t j = 0; j < 35; j++) {
            for (uint32_t i = 0; i < QUANTUM_PER_MS / 10; i++) {
                // unroll this loop to mitigate timing offset in wfi mode
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
                wait_quantum();
            }
            time_ms += 1;
        }

        if (led_gate > 0) {
          led_gate --;
        }
    }
}
