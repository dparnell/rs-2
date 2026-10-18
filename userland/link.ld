/* Layout of an OS/2 LX executable built with elf2lx: code and read-only
   data in one object at 64K, writable data in the next 64K-aligned one. */
ENTRY(_start)
SECTIONS {
    . = 0x10000;
    .text : { *(.text.start) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    . = ALIGN(0x10000);
    .data : { *(.data .data.*) *(.got) *(.got.plt) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment) }
}
