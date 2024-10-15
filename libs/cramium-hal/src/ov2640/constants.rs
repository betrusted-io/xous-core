#![allow(dead_code)]

/*
  The constant tables below are derived from https://github.com/STMicroelectronics/stm32-ov2640 with the following license:

    Copyright 2017 STMicroelectronics.
    All rights reserved.

    Redistribution and use in source and binary forms, with or without modification,
    are permitted provided that the following conditions are met:

    1. Redistributions of source code must retain the above copyright notice, this
    list of conditions and the following disclaimer.

    2. Redistributions in binary form must reproduce the above copyright notice,
    this list of conditions and the following disclaimer in the documentation and/or
    other materials provided with the distribution.

    3. Neither the name of the copyright holder nor the names of its contributors
    may be used to endorse or promote products derived from this software without
    specific prior written permission.

    THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
    ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
    WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
    DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
    ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
    (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
    LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
    ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
    (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
    SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

*/

/* OV2640 Registers definition when DSP bank selected (0xFF = 0x00) */
pub(crate) const OV2640_DSP_R_BYPASS: u8 = 0x05;
pub(crate) const OV2640_DSP_QS: u8 = 0x44;
pub(crate) const OV2640_DSP_CTRL: u8 = 0x50;
pub(crate) const OV2640_DSP_HSIZE1: u8 = 0x51;
pub(crate) const OV2640_DSP_VSIZE1: u8 = 0x52;
pub(crate) const OV2640_DSP_XOFFL: u8 = 0x53;
pub(crate) const OV2640_DSP_YOFFL: u8 = 0x54;
pub(crate) const OV2640_DSP_VHYX: u8 = 0x55;
pub(crate) const OV2640_DSP_DPRP: u8 = 0x56;
pub(crate) const OV2640_DSP_TEST: u8 = 0x57;
pub(crate) const OV2640_DSP_ZMOW: u8 = 0x5A;
pub(crate) const OV2640_DSP_ZMOH: u8 = 0x5B;
pub(crate) const OV2640_DSP_ZMHH: u8 = 0x5C;
pub(crate) const OV2640_DSP_BPADDR: u8 = 0x7C;
pub(crate) const OV2640_DSP_BPDATA: u8 = 0x7D;
pub(crate) const OV2640_DSP_CTRL2: u8 = 0x86;
pub(crate) const OV2640_DSP_CTRL3: u8 = 0x87;
pub(crate) const OV2640_DSP_SIZEL: u8 = 0x8C;
pub(crate) const OV2640_DSP_HSIZE2: u8 = 0xC0;
pub(crate) const OV2640_DSP_VSIZE2: u8 = 0xC1;
pub(crate) const OV2640_DSP_CTRL0: u8 = 0xC2;
pub(crate) const OV2640_DSP_CTRL1: u8 = 0xC3;
pub(crate) const OV2640_DSP_R_DVP_SP: u8 = 0xD3;
pub(crate) const OV2640_DSP_IMAGE_MODE: u8 = 0xDA;
pub(crate) const OV2640_DSP_RESET: u8 = 0xE0;
pub(crate) const OV2640_DSP_MS_SP: u8 = 0xF0;
pub(crate) const OV2640_DSP_SS_ID: u8 = 0x7F;
pub(crate) const OV2640_DSP_SS_CTRL: u8 = 0xF8;
pub(crate) const OV2640_DSP_MC_BIST: u8 = 0xF9;
pub(crate) const OV2640_DSP_MC_AL: u8 = 0xFA;
pub(crate) const OV2640_DSP_MC_AH: u8 = 0xFB;
pub(crate) const OV2640_DSP_MC_D: u8 = 0xFC;
pub(crate) const OV2640_DSP_P_STATUS: u8 = 0xFE;
pub(crate) const OV2640_DSP_RA_DLMT: u8 = 0xFF;

/* OV2640 Registers definition when sensor bank selected (0xFF = 0x01) */
pub(crate) const OV2640_SENSOR_GAIN: u8 = 0x00;
pub(crate) const OV2640_SENSOR_COM1: u8 = 0x03;
pub(crate) const OV2640_SENSOR_REG04: u8 = 0x04;
pub(crate) const OV2640_SENSOR_REG08: u8 = 0x08;
pub(crate) const OV2640_SENSOR_COM2: u8 = 0x09;
pub(crate) const OV2640_SENSOR_PIDH: u8 = 0x0A;
pub(crate) const OV2640_SENSOR_PIDL: u8 = 0x0B;
pub(crate) const OV2640_SENSOR_COM3: u8 = 0x0C;
pub(crate) const OV2640_SENSOR_COM4: u8 = 0x0D;
pub(crate) const OV2640_SENSOR_AEC: u8 = 0x10;
pub(crate) const OV2640_SENSOR_CLKRC: u8 = 0x11;
pub(crate) const OV2640_SENSOR_COM7: u8 = 0x12;
pub(crate) const OV2640_SENSOR_COM8: u8 = 0x13;
pub(crate) const OV2640_SENSOR_COM9: u8 = 0x14;
pub(crate) const OV2640_SENSOR_COM10: u8 = 0x15;
pub(crate) const OV2640_SENSOR_HREFST: u8 = 0x17;
pub(crate) const OV2640_SENSOR_HREFEND: u8 = 0x18;
pub(crate) const OV2640_SENSOR_VSTART: u8 = 0x19;
pub(crate) const OV2640_SENSOR_VEND: u8 = 0x1A;
pub(crate) const OV2640_SENSOR_MIDH: u8 = 0x1C;
pub(crate) const OV2640_SENSOR_MIDL: u8 = 0x1D;
pub(crate) const OV2640_SENSOR_AEW: u8 = 0x24;
pub(crate) const OV2640_SENSOR_AEB: u8 = 0x25;
pub(crate) const OV2640_SENSOR_W: u8 = 0x26;
pub(crate) const OV2640_SENSOR_REG2A: u8 = 0x2A;
pub(crate) const OV2640_SENSOR_FRARL: u8 = 0x2B;
pub(crate) const OV2640_SENSOR_ADDVSL: u8 = 0x2D;
pub(crate) const OV2640_SENSOR_ADDVHS: u8 = 0x2E;
pub(crate) const OV2640_SENSOR_YAVG: u8 = 0x2F;
pub(crate) const OV2640_SENSOR_REG32: u8 = 0x32;
pub(crate) const OV2640_SENSOR_ARCOM2: u8 = 0x34;
pub(crate) const OV2640_SENSOR_REG45: u8 = 0x45;
pub(crate) const OV2640_SENSOR_FLL: u8 = 0x46;
pub(crate) const OV2640_SENSOR_FLH: u8 = 0x47;
pub(crate) const OV2640_SENSOR_COM19: u8 = 0x48;
pub(crate) const OV2640_SENSOR_ZOOMS: u8 = 0x49;
pub(crate) const OV2640_SENSOR_COM22: u8 = 0x4B;
pub(crate) const OV2640_SENSOR_COM25: u8 = 0x4E;
pub(crate) const OV2640_SENSOR_BD50: u8 = 0x4F;
pub(crate) const OV2640_SENSOR_BD60: u8 = 0x50;
pub(crate) const OV2640_SENSOR_REG5D: u8 = 0x5D;
pub(crate) const OV2640_SENSOR_REG5E: u8 = 0x5E;
pub(crate) const OV2640_SENSOR_REG5F: u8 = 0x5F;
pub(crate) const OV2640_SENSOR_REG60: u8 = 0x60;
pub(crate) const OV2640_SENSOR_HISTO_LOW: u8 = 0x61;
pub(crate) const OV2640_SENSOR_HISTO_HIGH: u8 = 0x62;

/**
 * @brief  OV2640 Features Parameters
 */
pub(crate) const OV2640_BRIGHTNESS_LEVEL0: u8 = 0x40; /* Brightness level -2         */
pub(crate) const OV2640_BRIGHTNESS_LEVEL1: u8 = 0x30; /* Brightness level -1         */
pub(crate) const OV2640_BRIGHTNESS_LEVEL2: u8 = 0x20; /* Brightness level 0          */
pub(crate) const OV2640_BRIGHTNESS_LEVEL3: u8 = 0x10; /* Brightness level +1         */
pub(crate) const OV2640_BRIGHTNESS_LEVEL4: u8 = 0x00; /* Brightness level +2         */
pub(crate) const OV2640_BLACK_WHITE_BW: u8 = 0x18; /* Black and white effect      */
pub(crate) const OV2640_BLACK_WHITE_NEGATIVE: u8 = 0x40; /* Negative effect             */
pub(crate) const OV2640_BLACK_WHITE_BW_NEGATIVE: u8 = 0x58; /* BW and Negative effect      */
pub(crate) const OV2640_BLACK_WHITE_NORMAL: u8 = 0x00; /* Normal effect               */
pub(crate) const OV2640_CONTRAST_LEVEL0: u16 = 0x3418; /* Contrast level -2           */
pub(crate) const OV2640_CONTRAST_LEVEL1: u16 = 0x2A1C; /* Contrast level -2           */
pub(crate) const OV2640_CONTRAST_LEVEL2: u16 = 0x2020; /* Contrast level -2           */
pub(crate) const OV2640_CONTRAST_LEVEL3: u16 = 0x1624; /* Contrast level -2           */
pub(crate) const OV2640_CONTRAST_LEVEL4: u16 = 0x0C28; /* Contrast level -2           */
pub(crate) const OV2640_COLOR_EFFECT_ANTIQUE: u16 = 0xA640; /* Antique effect              */
pub(crate) const OV2640_COLOR_EFFECT_BLUE: u16 = 0x40A0; /* Blue effect                 */
pub(crate) const OV2640_COLOR_EFFECT_GREEN: u16 = 0x4040; /* Green effect                */
pub(crate) const OV2640_COLOR_EFFECT_RED: u16 = 0xC040; /* Red effect                  */

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColorMode {
    BlackWhite = OV2640_BLACK_WHITE_BW,
    Negative = OV2640_BLACK_WHITE_NEGATIVE,
    BlackWhiteNegative = OV2640_BLACK_WHITE_BW_NEGATIVE,
    Normal = OV2640_BLACK_WHITE_NORMAL,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Brightness {
    Level0 = OV2640_BRIGHTNESS_LEVEL0,
    Level1 = OV2640_BRIGHTNESS_LEVEL1,
    Level2 = OV2640_BRIGHTNESS_LEVEL2,
    Level3 = OV2640_BRIGHTNESS_LEVEL3,
    Level4 = OV2640_BRIGHTNESS_LEVEL4,
}

#[repr(u16)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Contrast {
    Level0 = OV2640_CONTRAST_LEVEL0,
    Level1 = OV2640_CONTRAST_LEVEL1,
    Level2 = OV2640_CONTRAST_LEVEL2,
    Level3 = OV2640_CONTRAST_LEVEL3,
    Level4 = OV2640_CONTRAST_LEVEL4,
}

#[repr(u16)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Antique = OV2640_COLOR_EFFECT_ANTIQUE,
    Blue = OV2640_COLOR_EFFECT_BLUE,
    Green = OV2640_COLOR_EFFECT_GREEN,
    Red = OV2640_COLOR_EFFECT_RED,
}
