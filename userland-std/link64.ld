/* Layout of a 64-bit OS/2 program built with Rust's std and rs2libc
   (docs/STD64.md): userland64/link64.ld's, with what link.ld has for the
   32-bit ones: the symbols the unwinder finds the tables by, and the
   thread-local storage template. Every PT_LOAD starts on 64 KB. */
ENTRY(_start)
SECTIONS {
    . = 0x10000;
    __executable_start = .;
    .text : { *(.text.start) *(.text .text.*) }
    __etext = .;
    .rodata : { *(.rodata .rodata.*) }
    .gcc_except_table : { *(.gcc_except_table .gcc_except_table.*) }
    .eh_frame_hdr : { __GNU_EH_FRAME_HDR = .; *(.eh_frame_hdr) }
    .eh_frame : { KEEP(*(.eh_frame)) }
    .dynsym : { *(.dynsym) }
    .dynstr : { *(.dynstr) }
    .hash : { *(.hash) }
    .rela.dyn : { *(.rela.dyn) *(.rela.plt) }
    . = ALIGN(0x10000);
    .data : { *(.data .data.*) }
    .init_array : {
        __init_array_start = .;
        KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
        KEEP(*(.init_array))
        __init_array_end = .;
    }
    .tdata : { __tdata_start = .; *(.tdata .tdata.*) __tdata_end = .; }
    .tbss : { __tbss_start = .; *(.tbss .tbss.*) __tbss_end = .; }
    __tls_align = MAX(ALIGNOF(.tdata), ALIGNOF(.tbss));
    .dynamic : { *(.dynamic) }
    .got : { *(.got) *(.got.plt) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.note*) *(.comment) }
}
