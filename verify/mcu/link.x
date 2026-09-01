ENTRY(_start)

MEMORY
{
  /* Rem MCU budget: 512 KB SRAM at the virt machine RAM base. */
  RAM (rwx) : ORIGIN = 0x80000000, LENGTH = 512K
}

SECTIONS
{
  .text : {
    *(.text.init)
    *(.text .text.*)
  } > RAM

  .rodata : {
    *(.rodata .rodata.*)
  } > RAM

  .data : {
    *(.data .data.*)
    *(.sdata .sdata.*)
  } > RAM

  .bss (NOLOAD) : {
    . = ALIGN(4);
    *(.bss .bss.*)
    *(.sbss .sbss.*)
    *(COMMON)
  } > RAM

  /* 8 KB reserved for the stack; the bump heap lives inside .bss. */
  _stack_top = ORIGIN(RAM) + LENGTH(RAM);
}
