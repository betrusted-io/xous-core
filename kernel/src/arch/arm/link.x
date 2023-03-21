MEMORY
{
    RAM : ORIGIN = 0xffd00000, LENGTH = 1M
}

ENTRY(reset);

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_RODATA", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);

SECTIONS
{
  .text :
  {
    /* Put reset handler first in .text section so it ends up as the entry */
    /* point of the program. */
    KEEP(*(.text.reset_vector));
    KEEP(*(.text.init));
    KEEP(*(.init));
    KEEP(*(.init.rust));
    . = ALIGN(4);
    KEEP(*(.trap));
    KEEP(*(.trap.rust));

    *(.text .text.*);
  } > REGION_TEXT

  .rodata : ALIGN(4)
  {
    *(.rodata .rodata.*);

    /* 4-byte align the end (VMA) of this section.
       This is required by LLD to ensure the LMA of the following .data
       section will have the correct alignment. */
    . = ALIGN(4);
    _etext = .;
  } > REGION_RODATA

  .data : ALIGN(4096)
  {
    _sidata = LOADADDR(.data);
    _sdata = .;
    *(.sdata .sdata.* .sdata2 .sdata2.*);
    *(.data .data.*);
    . = ALIGN(4);
    _edata = .;
  } > REGION_DATA AT > REGION_RODATA

  .bss (NOLOAD) :
  {
    _sbss = .;
    *(.sbss .sbss.* .bss .bss.*);
    . = ALIGN(4);
    _ebss = .;
  } > REGION_BSS

  /* fake output .got section */
  /* Dynamic relocations are unsupported. This section is only used to detect
     relocatable code in the input files and raise an error if relocatable code
     is found */
  .got (INFO) :
  {
    KEEP(*(.got .got.*));
  }

  /* Discard .eh_frame, we are not doing unwind on panic so it is not needed */
  /DISCARD/ :
  {
    *(.eh_frame);
    *(.eh_frame_hdr);
  }
}

PROVIDE(_romsize = _edata - _stext);
PROVIDE(_sramsize = _ebss - _stext);

/* Do not exceed this mark in the error messages above                                    | */
ASSERT(ORIGIN(RAM) % 4 == 0, "
ERROR(arm-rt): the start of the RAM must be 4-byte aligned");

ASSERT(_sdata % 4 == 0 && _edata % 4 == 0, "
BUG(arm-rt): .data is not 4-byte aligned");

ASSERT(_sbss % 4 == 0 && _ebss % 4 == 0, "
BUG(arm-rt): .bss is not 4-byte aligned");

ASSERT(SIZEOF(.got) == 0, "
.got section detected in the input files. Dynamic relocations are not
supported. If you are linking to C code compiled using the `gcc` crate
then modify your build script to compile the C code _without_ the
-fPIC flag. See the documentation of the `gcc::Config.fpic` method for
details.");

/* Do not exceed this mark in the error messages above                                    | */
