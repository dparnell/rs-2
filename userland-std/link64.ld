/* Layout of a 64-bit OS/2 program or DLL built with Rust's std and rs2libc
   (docs/STD64.md): userland64/link64.ld's, with what link.ld has for the
   32-bit ones: the thread-local storage template, and a description of
   the module for rs2libc (unwind tables, thread-local storage). Every PT_LOAD starts on 64 KB. */
ENTRY(_start)
SECTIONS {
    . = 0x10000;
    .text : { PROVIDE_HIDDEN(__text_start = .); *(.text.start) *(.text .text.*) PROVIDE_HIDDEN(__text_end = .); }
    .rodata : { *(.rodata .rodata.*) }
    .gcc_except_table : { *(.gcc_except_table .gcc_except_table.*) }
    .eh_frame_hdr : { PROVIDE_HIDDEN(__eh_frame_hdr_start = .); *(.eh_frame_hdr) }
    .eh_frame : { KEEP(*(.eh_frame)) }
    .dynsym : { *(.dynsym) }
    .dynstr : { *(.dynstr) }
    .hash : { *(.hash) }
    .rela.dyn : { *(.rela.dyn) *(.rela.plt) }
    . = ALIGN(0x10000);
    .data : { *(.data .data.*) }
    .init_array : {
        PROVIDE_HIDDEN(__init_array_start = .);
        KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
        KEEP(*(.init_array))
        PROVIDE_HIDDEN(__init_array_end = .);
    }
    .tdata : { PROVIDE_HIDDEN(__tdata_start = .); *(.tdata .tdata.*) PROVIDE_HIDDEN(__tdata_end = .); }
    .tbss : { *(.tbss .tbss.*) PROVIDE_HIDDEN(__tbss_end = .); }
    /* What rs2libc needs to know of the module (tls64.rs's Module): all
       relative to itself. */
    .rs2module : ALIGN(8) {
        RS2_MODULE = .;
        QUAD(__tdata_start - RS2_MODULE)
        QUAD(__tdata_end - __tdata_start)
        QUAD(__tbss_end - __tdata_start)
        QUAD(MAX(ALIGNOF(.tdata), ALIGNOF(.tbss)))
        QUAD(__text_start - RS2_MODULE)
        QUAD(__text_end - RS2_MODULE)
        QUAD(__eh_frame_hdr_start - RS2_MODULE)
        QUAD(__init_array_start - RS2_MODULE)
        QUAD(__init_array_end - RS2_MODULE)
        QUAD(0)
        /* A program's: the DLLs loaded with it that have thread-local
           storage (the loader fills this in, kernel elfload.rs). */
        . += 512;
    }
    .dynamic : { *(.dynamic) }
    .got : { *(.got) *(.got.plt) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.note*) *(.comment) }
}
