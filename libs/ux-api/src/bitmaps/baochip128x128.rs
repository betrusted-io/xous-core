#![cfg_attr(rustfmt, rustfmt_skip)]
pub const BITMAP: [u32; 512] = [
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffffffc0, 0xffffffff, 0xffffffff,
  0x1fffffff, 0xffff87c0, 0x01f0ffff, 0xfffffffc,
  0x1fffffff, 0xfff80040, 0x01000fff, 0xfffffffc,
  0x1fffffff, 0xffe00000, 0x000003ff, 0xfffffffc,
  0x1fffffff, 0xffc00000, 0x000000ff, 0xfffffffc,
  0x1fffffff, 0xff000000, 0x0000007f, 0xfffffffc,
  0x1fffffff, 0xfe000000, 0x0000003f, 0xfffffffc,
  0x1fffffff, 0xfe000000, 0x0000001f, 0xfffffffc,
  0x1fffffff, 0xf8000000, 0x0000000f, 0xfffffffc,
  0x1fffffff, 0xf8000000, 0x0000000f, 0xfffffffc,
  0x1fffffff, 0xf0003000, 0x000f8007, 0xfffffffc,
  0x1fffffff, 0xf000fc00, 0x003fc007, 0xfffffffc,
  0x1fffffff, 0xf003fe00, 0x007fe007, 0xfffffffc,
  0x1fffffff, 0xe007ff80, 0x007ff003, 0xfffffffc,
  0x1fffffff, 0xe007ff80, 0x00fff803, 0xfffffffc,
  0x1fffffff, 0xe00fff80, 0x00fff803, 0xfffffffc,
  0x1fffffff, 0xe00fff80, 0x00fff803, 0xfffffffc,
  0x1fffffff, 0xe00fff80, 0x00fff803, 0xfffffffc,
  0x1fffffff, 0xe00fff80, 0x00fff803, 0xfffffffc,
  0x1fffffff, 0xe007ff80, 0x00fff003, 0xfffffffc,
  0x1fffffff, 0xe007ff80, 0x007ff003, 0xfffffffc,
  0x1fffffff, 0xf003ff00, 0x003fc007, 0xfffffffc,
  0x1fffffff, 0xf001fe00, 0x001f8007, 0xfffffffc,
  0x1fffffff, 0xf0007800, 0x00000007, 0xfffffffc,
  0x1fffffff, 0xf8000000, 0x0000000f, 0xfffffffc,
  0x1fffffff, 0xf8000000, 0x0000000f, 0xfffffffc,
  0x1fffffff, 0xfc000000, 0x0000001f, 0xfffffffc,
  0x1fffffff, 0xfe000000, 0x0000003f, 0xfffffffc,
  0x1fffffff, 0xff000000, 0x0000007f, 0xfffffffc,
  0x1fffffff, 0xff800000, 0x000000ff, 0xfffffffc,
  0x1fffffff, 0xffc00000, 0x000001ff, 0xfffffffc,
  0x1fffffff, 0xfff00000, 0x000007ff, 0xfffffffc,
  0x1fffffff, 0xffff03c0, 0x01e07fff, 0xfffffffc,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffe3871f, 0xffe03fff, 0xffffffff,
  0xffffffff, 0xffe3871f, 0xff800fff, 0xffffffff,
  0xffffffff, 0xff000003, 0xfc0003ff, 0xffffffff,
  0xffffffff, 0xfe000001, 0xf80000ff, 0xffffffff,
  0xffffffff, 0xfc000000, 0xe000007f, 0xffffffff,
  0xffffffff, 0xfc000000, 0xe000003f, 0xffffffff,
  0xffffffff, 0xfc000000, 0xc000001f, 0xffffffff,
  0x3fffffff, 0xf0000000, 0x8000001f, 0xffffffff,
  0x3fffffff, 0xf0000000, 0x8000000f, 0xffffffff,
  0x3fffffff, 0xf0000000, 0x000f000f, 0xffffffff,
  0xffffffff, 0xfc000000, 0x003fc007, 0xffffffff,
  0xffffffff, 0xfc000000, 0x007ff007, 0xfffffffe,
  0xffffffff, 0xfc000000, 0x007ff007, 0xfffffffe,
  0x3fffffff, 0xf0000000, 0x00fff007, 0xfffffffe,
  0x3fffffff, 0xf0000000, 0x00fff803, 0xfffffffe,
  0x3fffffff, 0xf0000000, 0x00fff803, 0xfffffffe,
  0x3fffffff, 0xf0000000, 0x00fff803, 0xfffffffe,
  0xffffffff, 0xfc000000, 0x00fff803, 0xfffffffe,
  0xffffffff, 0xfc000000, 0x00fff007, 0xfffffffe,
  0xffffffff, 0xfc000000, 0x007ff007, 0xfffffffe,
  0x3fffffff, 0xf0000000, 0x003fe007, 0xfffffffe,
  0x3fffffff, 0xf0000000, 0x001f8007, 0xffffffff,
  0x3fffffff, 0xf0000000, 0x8000000f, 0xffffffff,
  0xffffffff, 0xfc000000, 0x8000000f, 0xffffffff,
  0xffffffff, 0xfc000000, 0xc000001f, 0xffffffff,
  0xffffffff, 0xfc000000, 0xc000003f, 0xffffffff,
  0xffffffff, 0xfe000001, 0xe000007f, 0xffffffff,
  0xffffffff, 0xff000003, 0xf00000ff, 0xffffffff,
  0xffffffff, 0xffe3871f, 0xf80001ff, 0xffffffff,
  0xffffffff, 0xffe3871f, 0xfe0007ff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffc03fff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xfffffff1,
  0xfff1ffff, 0xffffffff, 0xfff8ffff, 0xfffffff1,
  0xfff1ffff, 0xffffffff, 0xfff8ffff, 0xfffffff1,
  0xfff1ffff, 0xffffffff, 0xfff8ffff, 0xffffffff,
  0xfff1ffff, 0xffffffff, 0xfff8ffff, 0xffffffff,
  0xfff1ffff, 0xffffffff, 0xfff8ffff, 0xffffffff,
  0xf011ffff, 0xf01f8c0f, 0xfc08f80f, 0xfffc08f1,
  0xe001ffff, 0xe00f8007, 0xf800f007, 0xfff800f1,
  0xc181ffff, 0xc38781c3, 0xf000e1c3, 0xfff0e0f1,
  0x87e1ffff, 0x87c383e1, 0xe1f0e3e1, 0xffe1f0f1,
  0x8fe1ffff, 0x8fe387f1, 0xe3f8fff1, 0xffe3f8f1,
  0x8ff1ffff, 0x8fe38ff1, 0xe3f8fff1, 0xffe3f8f1,
  0x8ff1ffff, 0x8fe38ff1, 0xe3f8fff1, 0xffe3f8f1,
  0x8fe1ffff, 0x8fe38ff1, 0xe3f8fff1, 0xffe3f8f1,
  0x87e1ffff, 0x8fe387f1, 0xe3f8ffe1, 0xffe3f8f1,
  0xc3c1ffff, 0x87c383e3, 0xe3f8e3c3, 0xffe1f0f1,
  0xc001ffff, 0xc0078003, 0xe3f8e007, 0xfff000f1,
  0xe011ffff, 0xe00f8807, 0xe3f8f00f, 0xfff800f1,
  0xfc7fffff, 0xfc7ffe3f, 0xfffffc3f, 0xfffe18ff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xfffff8ff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xfffff8ff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xfffff8ff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xfffff8ff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,
  0xffffffff, 0xffffffff, 0xffffffff, 0xffffffff,

];
