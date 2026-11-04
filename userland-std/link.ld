/* Layout of an OS/2 LX executable built with Rust's std and rs2libc (see
   userland/link.ld for the no_std programs). Code, read-only data and the
   unwind tables (.eh_frame, found through .eh_frame_hdr) in one object at
   64K; writable data, the thread-local storage template (.tdata/.tbss) and
   the constructors (.init_array, which std uses to receive argc/argv) in
   the next 64K-aligned one. */
ENTRY(_start)
SECTIONS {
    . = 0x10000;
    __executable_start = .;
    .text : { *(.text.start) *(.text .text.*) }
    __etext = .;
    .rodata : { *(.rodata .rodata.*) }
    .eh_frame_hdr : { __GNU_EH_FRAME_HDR = .; *(.eh_frame_hdr) }
    .eh_frame : { KEEP(*(.eh_frame)) }
    .gcc_except_table : { *(.gcc_except_table .gcc_except_table.*) }
    . = ALIGN(0x10000);
    .data : { *(.data .data.*) }
    .data.rel.ro : { *(.data.rel.ro .data.rel.ro.*) }
    .got : { *(.got) *(.got.plt) }
    .init_array : {
        __init_array_start = .;
        KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*)))
        KEEP(*(.init_array))
        __init_array_end = .;
    }
    .tdata : { __tdata_start = .; *(.tdata .tdata.*) __tdata_end = .; }
    .tbss : { __tbss_start = .; *(.tbss .tbss.*) __tbss_end = .; }
    __tls_align = MAX(ALIGNOF(.tdata), ALIGNOF(.tbss));
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.note*) *(.comment) }
}
