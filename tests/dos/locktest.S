// LOCKTEST.COM: INT 21h AH=5Ch byte-range locks in a DOS session, which
// are OS/2's mandatory locks: a file opened twice (two SFTs), a lock
// through the first handle stops a read and a lock through the second
// (error 33, nothing read), the unlock lets them through. Under OS/2 only
// (dosrun's host has no locks). Prints "LOCKTEST OK" or
// "LOCKTEST FAILED x" (x: the step that went wrong, A = 1).
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
// Fail with step \n unless CF and AX are as expected.
.macro expect_ok n
    mov byte ptr [step], \n
    jc fail
.endm
.macro expect_err n, code
    mov byte ptr [step], \n
    jnc fail
    cmp ax, \code
    jne fail
.endm
.text
.globl _start
.org 0x100
_start:
    // Create the file with 512 bytes.
    mov ah, 0x3c
    xor cx, cx
    mov dx, offset fname
    int 0x21
    expect_ok 1
    mov bx, ax
    mov ah, 0x40
    mov cx, 512
    mov dx, offset buf
    int 0x21
    expect_ok 2
    mov ah, 0x3e
    int 0x21
    // Open it twice, read/write, deny none.
    mov ax, 0x3d42
    mov dx, offset fname
    int 0x21
    expect_ok 3
    mov [h1], ax
    mov ax, 0x3d42
    mov dx, offset fname
    int 0x21
    expect_ok 4
    mov [h2], ax
    // Lock [100, 200) through h1.
    mov ax, 0x5c00
    mov bx, [h1]
    xor cx, cx
    mov dx, 100
    xor si, si
    mov di, 100
    int 0x21
    expect_ok 5
    // h2: reading [50, 150) fails, nothing read.
    mov bx, [h2]
    mov dx, 50
    call16 seek
    mov ah, 0x3f
    mov cx, 100
    mov dx, offset buf
    int 0x21
    expect_err 6, 33
    // h2: [0, 50) reads.
    mov bx, [h2]
    xor dx, dx
    call16 seek
    mov ah, 0x3f
    mov cx, 50
    mov dx, offset buf
    int 0x21
    expect_ok 7
    cmp ax, 50
    jne fail
    // h2 cannot lock inside h1's lock.
    mov ax, 0x5c00
    mov bx, [h2]
    xor cx, cx
    mov dx, 150
    xor si, si
    mov di, 10
    int 0x21
    expect_err 8, 33
    // h1 reads and writes its own lock.
    mov bx, [h1]
    mov dx, 120
    call16 seek
    mov ah, 0x40
    mov cx, 10
    mov dx, offset buf
    int 0x21
    expect_ok 9
    // Zero length, bad subfunction.
    mov ax, 0x5c00
    mov bx, [h1]
    xor cx, cx
    mov dx, 300
    xor si, si
    xor di, di
    int 0x21
    expect_err 10, 33
    mov ax, 0x5c02
    mov bx, [h1]
    int 0x21
    expect_err 11, 1
    // Unlock; h2 reads [50, 150) now; a second unlock fails.
    mov ax, 0x5c01
    mov bx, [h1]
    xor cx, cx
    mov dx, 100
    xor si, si
    mov di, 100
    int 0x21
    expect_ok 12
    mov bx, [h2]
    mov dx, 50
    call16 seek
    mov ah, 0x3f
    mov cx, 100
    mov dx, offset buf
    int 0x21
    expect_ok 13
    cmp ax, 100
    jne fail
    mov ax, 0x5c01
    mov bx, [h1]
    xor cx, cx
    mov dx, 100
    xor si, si
    mov di, 100
    int 0x21
    expect_err 14, 33
    // Lock again through h1 and close it: the lock goes with the handle.
    mov ax, 0x5c00
    mov bx, [h1]
    xor cx, cx
    mov dx, 100
    xor si, si
    mov di, 100
    int 0x21
    expect_ok 15
    mov ah, 0x3e
    mov bx, [h1]
    int 0x21
    mov ax, 0x5c00
    mov bx, [h2]
    xor cx, cx
    mov dx, 150
    xor si, si
    mov di, 10
    int 0x21
    expect_ok 16
    mov ah, 0x3e
    mov bx, [h2]
    int 0x21
    mov ah, 0x41
    mov dx, offset fname
    int 0x21
    mov dx, offset s_ok
    mov ah, 9
    int 0x21
    mov ax, 0x4c00
    int 0x21

fail:
    mov dx, offset s_fail
    mov ah, 9
    int 0x21
    mov dl, [step]
    add dl, '@'
    mov ah, 2
    int 0x21
    mov dx, offset s_crlf
    mov ah, 9
    int 0x21
    mov ax, 0x4c01
    int 0x21

// Move handle BX to offset DX.
seek:
    mov ax, 0x4200
    xor cx, cx
    int 0x21
    .byte 0xc3

fname: .asciz "LOCKT.TMP"
s_ok: .ascii "LOCKTEST OK\r\n$"
s_fail: .ascii "LOCKTEST FAILED $"
s_crlf: .ascii "\r\n$"
step: .byte 0
h1: .word 0
h2: .word 0
buf: .fill 512, 1, 0x55
