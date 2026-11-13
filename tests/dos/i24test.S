// I24TEST.COM: sharing and lock violations in a DOS session: the retries
// (INT 21h 440Bh) and the critical error (INT 24h) for compatibility-mode
// handles and FCBs. A file opened twice: through a deny-none handle a
// region is locked; a compatibility-mode handle and an FCB reading or
// writing it get INT 24h (the program's handler counts the calls, checks
// AH/AL/DI, the caller's AX in DOS's stack frame and the extended error,
// and answers from a list: Fail, Retry then Fail, Ignore), a deny-none
// handle gets error 21h without INT 24h. Then sharing violations on
// opens: a compatibility-mode open and an FCB open of a file open
// deny-read/write raise INT 24h, a deny-none open gets 20h.
// Prints "I24TEST OK" or "I24TEST FAILED x" (x: the step, A = 1).
//
// With "D" on the command line: DOS's own INT 24h handler (no handler of
// the program's) for a compatibility-mode read of the locked region:
// answer R then F at its prompts; prints "I24TEST DEFAULT OK". With "A":
// answer A, which ends the program (nothing more is printed).
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
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
// Fail with step \n unless the byte/word at \var is \val.
.macro check_b n, var, val
    mov byte ptr [step], \n
    cmp byte ptr [\var], \val
    jne fail
.endm
.macro check_w n, var, val
    mov byte ptr [step], \n
    cmp word ptr [\var], \val
    jne fail
.endm
// Read (AH=3Fh) or write (40h) CX bytes of handle BX at offset DX.
.macro rw_at fn, h, off, len
    mov bx, [\h]
    mov dx, \off
    call16 seek
    mov ax, \fn << 8
    mov bx, [\h]
    mov cx, \len
    mov dx, offset buf
    int 0x21
.endm
.text
.globl _start
.org 0x100
_start:
    // The command tail's first letter (the mode).
    mov si, 0x81
1:  lodsb
    cmp al, ' '
    je 1b
    and al, 0xdf
    mov [mode], al
    // The current drive (AL of INT 24h).
    mov ah, 0x19
    int 0x21
    mov [drive], al
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
    expect_ok 1
    mov ah, 0x3e
    int 0x21
    // h1: deny none; h2: compatibility mode; h3: deny none.
    mov ax, 0x3d42
    mov dx, offset fname
    int 0x21
    expect_ok 2
    mov [h1], ax
    mov ax, 0x3d02
    mov dx, offset fname
    int 0x21
    expect_ok 2
    mov [h2], ax
    mov ax, 0x3d42
    mov dx, offset fname
    int 0x21
    expect_ok 2
    mov [h3], ax
    // Lock [100, 200) through h1.
    mov ax, 0x5c00
    mov bx, [h1]
    xor cx, cx
    mov dx, 100
    xor si, si
    mov di, 100
    int 0x21
    expect_ok 3
    cmp byte ptr [mode], 'D'
    je default_handler
    cmp byte ptr [mode], 'A'
    je default_handler
    // 440Bh: no tries is an error; 2 tries, no delay.
    mov ax, 0x440b
    xor dx, dx
    mov cx, 1
    int 0x21
    expect_err 4, 1
    mov ax, 0x440b
    mov dx, 2
    xor cx, cx
    int 0x21
    expect_ok 4
    // Our INT 24h handler.
    mov ax, 0x2524
    mov dx, offset i24
    int 0x21

    // h2 (compatibility) reads [50, 150): INT 24h, Fail -> 53h.
    mov word ptr [answers], offset ans_fail
    rw_at 0x3f, h2, 50, 100
    expect_err 5, 0x53
    check_w 6, count, 1
    check_b 7, s_ah, 0x1e
    mov al, [drive]
    mov byte ptr [step], 8
    cmp [s_al], al
    jne fail
    check_w 9, s_di, 0x0c
    check_w 10, s_cax, 0x3f00
    check_w 11, s_ext, 0x21
    // The extended error after the call: 53h.
    mov ah, 0x59
    xor bx, bx
    int 0x21
    mov byte ptr [step], 12
    cmp ax, 0x53
    jne fail
    // h2 writes [120, 130): Retry (the call again, INT 24h again), then
    // Fail.
    mov word ptr [answers], offset ans_retry
    rw_at 0x40, h2, 120, 10
    expect_err 13, 0x53
    check_w 13, count, 3
    check_b 14, s_ah, 0x1f
    check_w 14, s_cax, 0x4000
    // Ignore is not allowed: Fail.
    mov word ptr [answers], offset ans_ignore
    rw_at 0x3f, h2, 150, 10
    expect_err 15, 0x53
    check_w 15, count, 4
    // h2 outside the lock: fine.
    rw_at 0x3f, h2, 300, 100
    expect_ok 16
    cmp ax, 100
    jne fail
    // h3 (deny none): error 21h, no INT 24h.
    rw_at 0x3f, h3, 50, 100
    expect_err 17, 0x21
    rw_at 0x40, h3, 120, 10
    expect_err 17, 0x21
    check_w 17, count, 4

    // An FCB on the file: record 1 (128-255) is locked.
    mov ah, 0x0f
    mov dx, offset fcb
    int 0x21
    mov byte ptr [step], 18
    cmp al, 0
    jne fail
    mov ah, 0x1a
    mov dx, offset buf
    int 0x21
    mov word ptr [answers], offset ans_fail
    mov word ptr [fcb + 0x21], 1
    mov word ptr [fcb + 0x23], 0
    mov ax, 0x2100
    mov dx, offset fcb
    int 0x21
    mov byte ptr [step], 19
    cmp al, 1
    jne fail
    check_w 19, count, 5
    check_b 19, s_ah, 0x1e
    check_w 19, s_cax, 0x2100
    mov word ptr [answers], offset ans_fail
    mov ax, 0x2200
    mov dx, offset fcb
    int 0x21
    mov byte ptr [step], 20
    cmp al, 1
    jne fail
    check_w 20, count, 6
    check_b 20, s_ah, 0x1f
    // Record 3 (384-511) reads.
    mov word ptr [fcb + 0x21], 3
    mov ax, 0x2100
    mov dx, offset fcb
    int 0x21
    mov byte ptr [step], 21
    cmp al, 0
    jne fail
    check_w 21, count, 6
    mov ah, 0x10
    mov dx, offset fcb
    int 0x21

    // Sharing violations on opens: close everything, open deny
    // read/write.
    mov bx, [h1]
    mov ah, 0x3e
    int 0x21
    mov bx, [h2]
    mov ah, 0x3e
    int 0x21
    mov bx, [h3]
    mov ah, 0x3e
    int 0x21
    mov ax, 0x3d10
    mov dx, offset fname
    int 0x21
    expect_ok 22
    mov [h1], ax
    // Deny none: 20h, no INT 24h.
    mov ax, 0x3d40
    mov dx, offset fname
    int 0x21
    expect_err 23, 0x20
    check_w 23, count, 6
    // Compatibility mode: INT 24h, Fail -> 53h.
    mov word ptr [answers], offset ans_fail
    mov ax, 0x3d00
    mov dx, offset fname
    int 0x21
    expect_err 24, 0x53
    check_w 24, count, 7
    check_w 24, s_ext, 0x20
    check_w 24, s_cax, 0x3d00
    // An FCB open: INT 24h, AL = FFh.
    mov word ptr [answers], offset ans_fail
    mov ah, 0x0f
    mov dx, offset fcb
    int 0x21
    mov byte ptr [step], 25
    cmp al, 0xff
    jne fail
    check_w 25, count, 8
    check_w 25, s_ext, 0x20
    mov ah, 0x3e
    mov bx, [h1]
    int 0x21
    mov ah, 0x41
    mov dx, offset fname
    int 0x21
    mov dx, offset s_ok
    jmp done

// DOS's own handler: a compatibility-mode read in the lock.
default_handler:
    rw_at 0x3f, h2, 50, 100
    expect_err 26, 0x53
    mov dx, offset s_default
done:
    push dx
    mov ah, 0x3e
    mov bx, [h1]
    int 0x21
    mov ah, 0x3e
    mov bx, [h2]
    int 0x21
    mov ah, 0x3e
    mov bx, [h3]
    int 0x21
    mov ah, 0x41
    mov dx, offset fname
    int 0x21
    pop dx
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
    mov ah, 0x3e
    mov bx, [h1]
    int 0x21
    mov ah, 0x3e
    mov bx, [h2]
    int 0x21
    mov ah, 0x3e
    mov bx, [h3]
    int 0x21
    mov ah, 0x41
    mov dx, offset fname
    int 0x21
    mov ax, 0x4c01
    int 0x21

// Move handle BX to offset DX.
seek:
    mov ax, 0x4200
    xor cx, cx
    int 0x21
    .byte 0xc3

// INT 24h: count, note AH AL DI, the caller's AX (DOS's frame: our IRET
// frame, then AX BX CX DX SI DI BP DS ES) and the extended error; answer
// with the next byte of the list at [answers].
i24:
    push bp
    mov bp, sp
    push ds
    push es
    push ax
    push bx
    push cx
    push dx
    push si
    push di
    push cs
    pop ds
    inc word ptr [count]
    mov [s_ah], ah
    mov [s_al], al
    mov [s_di], di
    mov ax, [bp + 8]
    mov [s_cax], ax
    mov ah, 0x59
    xor bx, bx
    int 0x21
    push cs
    pop ds
    mov [s_ext], ax
    mov bx, [answers]
    mov al, [bx]
    mov [answer], al
    inc word ptr [answers]
    pop di
    pop si
    pop dx
    pop cx
    pop bx
    pop ax
    mov al, cs:[answer]
    pop es
    pop ds
    pop bp
    .byte 0xcf

fname: .asciz "CRITT.TMP"
// FCB: default drive, CRITT.TMP.
fcb: .byte 0
    .ascii "CRITT   TMP"
    .fill 25, 1, 0
s_ok: .ascii "I24TEST OK\r\n$"
s_default: .ascii "I24TEST DEFAULT OK\r\n$"
s_fail: .ascii "I24TEST FAILED $"
s_crlf: .ascii "\r\n$"
// Answers: 3 Fail, 1 Retry, 0 Ignore.
ans_fail: .byte 3
ans_retry: .byte 1, 3
ans_ignore: .byte 0
step: .byte 0
mode: .byte 0
drive: .byte 0
s_ah: .byte 0
s_al: .byte 0
answer: .byte 0
.p2align 1
answers: .word 0
count: .word 0
s_di: .word 0
s_cax: .word 0
s_ext: .word 0
h1: .word 0
h2: .word 0
h3: .word 0
buf: .fill 512, 1, 0x55
