ENTRY(_start)

SECTIONS {
    . = 0xFFFFFFFF80000000;
    __kernel_start = .;

    .text : ALIGN(4K) {
        *(.text.entry)
        *(.text .text.*)
    }
    __text_end = .;
    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    }
    .data : ALIGN(4K) {
        *(.data .data.*)
    }
    .bss : ALIGN(4K) {
        *(.bss .bss.*)
        *(COMMON)
    }
    __kernel_end = ALIGN(4K);

    /DISCARD/ : {
        *(.eh_frame*)
        *(.comment)
    }
}
