MEMORY
{
  /* Actual flash: */
  /* FLASH : ORIGIN = 0x08000000, LENGTH = 128K */

  /* Bootloader flash: */
  FLASH : ORIGIN = 0x08000000, LENGTH = 64K 
  
  /* Shifted 64k to make room for debug UF2 bootloader */
  /*
  FLASH : ORIGIN = 0x08010000, LENGTH = 64K
  */

  /* Shifted 16k to make room for UF2 bootloader */
  /* FLASH : ORIGIN = 0x08004000, LENGTH = 112K */

  RAM : ORIGIN = 0x20000000, LENGTH = 20K
}