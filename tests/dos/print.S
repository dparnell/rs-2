// PRINT.COM: printer output by every DOS way: the standard printer
// handle (4), "LPT1" opened as a file, INT 21h 05h, INT 17h and the
// parallel port's registers (data at 378h, STROBE in 37Ah). In an OS/2
// DOS session it all becomes one print job (docs/PRINTING.md); dosrun
// writes it to its --lpt file.
.intel_syntax noprefix
.code16
.text
.globl _start
.org 0x100
_start:
    // The BIOS lists LPT1 at 378h.
    xor ax, ax
    mov es, ax
    mov ax, es:[0x408]
    cmp ax, 0x378
    jne fail
    // INT 17h status: selected, not busy.
    mov ah, 2
    xor dx, dx
    int 0x17
    test ah, 0x80
    jz fail
    // 1. Handle 4 (stdprn).
    mov ah, 0x40
    mov bx, 4
    mov cx, offset l1e
    sub cx, offset l1
    mov dx, offset l1
    int 0x21
    jc fail
    // 2. LPT1 as a file.
    mov ax, 0x3d01
    mov dx, offset lptname
    int 0x21
    jc fail
    mov bx, ax
    mov ah, 0x40
    mov cx, offset l2e
    sub cx, offset l2
    mov dx, offset l2
    int 0x21
    jc fail
    mov ah, 0x3e
    int 0x21
    // 3. INT 21h 05h, a character at a time.
    mov si, offset l3
1:  lodsb
    test al, al
    jz 2f
    mov dl, al
    mov ah, 5
    int 0x21
    jmp 1b
    // 4. INT 17h 00h.
2:  mov si, offset l4
3:  lodsb
    test al, al
    jz 4f
    mov ah, 0
    xor dx, dx
    int 0x17
    test ah, 0x29
    jnz fail
    jmp 3b
    // 5. The port: data, then pulse STROBE.
4:  mov si, offset l5
5:  lodsb
    test al, al
    jz 6f
    mov dx, 0x378
    out dx, al
    mov dx, 0x379
7:  in al, dx
    test al, 0x80
    jz 7b
    mov dx, 0x37a
    mov al, 0x0d
    out dx, al
    mov al, 0x0c
    out dx, al
    jmp 5b
6:  mov ah, 9
    mov dx, offset ok
    int 0x21
    mov ax, 0x4c00
    int 0x21
fail:
    mov ah, 9
    mov dx, offset bad
    int 0x21
    mov ax, 0x4c01
    int 0x21
l1:   .ascii "1 stdprn\r\n"
l1e:
l2:   .ascii "2 LPT1 file\r\n"
l2e:
l3:   .asciz "3 INT 21h 05h\r\n"
l4:   .asciz "4 INT 17h\r\n"
l5:   .asciz "5 port 378h\r\n\f"
lptname: .asciz "LPT1"
ok:   .ascii "PRINT OK\r\n$"
bad:  .ascii "PRINT FAILED\r\n$"
