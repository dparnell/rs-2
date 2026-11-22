/* Layout of an rs2 64-bit module (docs/API64.md): read-only and code in
   one segment at 64K, writable data in the next 64K-aligned one; every
   PT_LOAD starts on 64 KB, as the loader's objects do. */
ENTRY(_start)
SECTIONS {
    /* DLLs pass --defsym=__image_base=... to link in the shared arena. */
    . = DEFINED(__image_base) ? __image_base : 0x10000;
    .text : { *(.text.start) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .dynsym : { *(.dynsym) }
    .dynstr : { *(.dynstr) }
    .hash : { *(.hash) }
    .rela.dyn : { *(.rela.dyn) *(.rela.plt) }
    . = ALIGN(0x10000);
    .data : { *(.data .data.*) }
    /* C constructors and destructors (the C library runs them). */
    .init_array : {
        PROVIDE_HIDDEN(__init_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
        KEEP(*(.init_array))
        PROVIDE_HIDDEN(__init_array_end = .);
    }
    .fini_array : {
        PROVIDE_HIDDEN(__fini_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*)))
        KEEP(*(.fini_array))
        PROVIDE_HIDDEN(__fini_array_end = .);
    }
    .dynamic : { *(.dynamic) }
    .got : { *(.got) *(.got.plt) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /* DLL data shared by all processes (#[link_section = ".shared"]). */
    . = ALIGN(0x10000);
    .shared : { *(.shared .shared.*) }
    /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment) }
}
