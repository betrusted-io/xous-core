#![allow(dead_code)]
/*
  The constant tables below are derived from https://github.com/zephyrproject-rtos/zephyr/blob/main/drivers/video/gc2145.c
  with the following license:

 * Copyright (c) 2024 Felipe Neves
 *
 * SPDX-License-Identifier: Apache-2.0
*/

pub(crate) const GC2145_REG_AMODE1: u8 = 0x17;
pub(crate) const GC2145_AMODE1_WINDOW_MASK: u8 = 0xFC;
pub(crate) const GC2145_REG_AMODE1_DEF: u8 = 0x14;
pub(crate) const GC2145_REG_OUTPUT_FMT: u8 = 0x84;
pub(crate) const GC2145_REG_OUTPUT_FMT_MASK: u8 = 0x1F;
pub(crate) const GC2145_REG_OUTPUT_FMT_RGB565: u8 = 0x06;
pub(crate) const GC2145_REG_OUTPUT_FMT_YCBYCR: u8 = 0x02;
pub(crate) const GC2145_REG_SYNC_MODE: u8 = 0x86;
pub(crate) const GC2145_REG_SYNC_MODE_DEF: u8 = 0x23;
pub(crate) const GC2145_REG_SYNC_MODE_COL_SWITCH: u8 = 0x10;
pub(crate) const GC2145_REG_SYNC_MODE_ROW_SWITCH: u8 = 0x20;
pub(crate) const GC2145_REG_RESET: u8 = 0xFE;
pub(crate) const GC2145_REG_SW_RESET: u8 = 0x80;
pub(crate) const GC2145_SET_P0_REGS: u8 = 0x00;
pub(crate) const GC2145_REG_CROP_ENABLE: u8 = 0x90;
pub(crate) const GC2145_CROP_SET_ENABLE: u8 = 0x01;
pub(crate) const GC2145_REG_BLANK_WINDOW_BASE: u8 = 0x09;
pub(crate) const GC2145_REG_WINDOW_BASE: u8 = 0x91;
pub(crate) const GC2145_REG_SUBSAMPLE: u8 = 0x99;
pub(crate) const GC2145_REG_SUBSAMPLE_MODE: u8 = 0x9A;
pub(crate) const GC2145_SUBSAMPLE_MODE_SMOOTH: u8 = 0x0E;
pub(crate) const GC2145_PIDH: u8 = 0xF0;
pub(crate) const GC2145_PIDL: u8 = 0xF1;
pub(crate) const GC2145_I2C_ID: u8 = 0xFB;

pub(crate) const UXGA_HSIZE: u16 = 1600;
pub(crate) const UXGA_VSIZE: u16 = 1200;
