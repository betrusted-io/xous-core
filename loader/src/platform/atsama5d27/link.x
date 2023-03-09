MEMORY
{
  RAM : ORIGIN = 0x20000000, LENGTH = 1M
}

PROVIDE(_mem_start = ORIGIN(RAM));                    /* Start of the memory */
PROVIDE(_top_of_memory = ORIGIN(RAM) + LENGTH(RAM));  /* Top of the memory */

ENTRY(reset);

REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_RODATA", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_STACK", RAM);
REGION_ALIAS("REGION_HEAP", RAM);

/* Size of the main kernel stack */
_stack_size = 16K;
_eheap = ORIGIN(RAM) + LENGTH(RAM);

PROVIDE(_stext = ORIGIN(REGION_TEXT));
PROVIDE(_stack_start = ORIGIN(REGION_STACK) + LENGTH(REGION_STACK));
PROVIDE(_max_hart_id = 0);
PROVIDE(_hart_stack_size = 2K);
PROVIDE(_heap_size = 0);

SECTIONS
{
  . = ALIGN(4);
  .text.dummy (NOLOAD) :
  {
    /* This section is intended to make _stext address work */
    . = ABSOLUTE(_stext);
  } > REGION_TEXT

  .text _stext :
  {
    *(.text);    /* Place .text section first so that the reset vector is always placed at the load address */
    *(.text.*);  /* Place other code sections next */
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

  .data : ALIGN(4)
  {
    _sidata = LOADADDR(.data);
    _sdata = .;
    /* Must be called __global_pointer$ for linker relaxations to work. */
    PROVIDE(__global_pointer$ = . + 0x800);
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

  /* fictitious region that represents the memory available for the stack */
  .stack (NOLOAD) :
  {
    _sstack = .;
    . += _stack_size;
    . = ALIGN(4096);
    _estack = .;
  } > REGION_STACK

  /* fictitious region that represents the memory available for the heap */
  .heap (NOLOAD) :
  {
    . = ALIGN(4);
    _sheap = .;
    /* _eheap is defined elsewhere and is the remainder of RAM */
  } > REGION_HEAP

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

end = .;  /* define a global symbol marking the end of application */

/* Do not exceed this mark in the error messages below                                    | */
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
