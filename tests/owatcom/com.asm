; A DOS .COM program for exe2bin: wasm com.asm, wlink system dos com,
; exe2bin com.exe com.com. It writes COM.OUT and exits.
.8086
_TEXT   segment para public 'CODE'
        assume  cs:_TEXT, ds:_TEXT
        org     100h
start:  mov     ah, 3Ch         ; create COM.OUT
        xor     cx, cx
        mov     dx, offset fname
        int     21h
        jc      done
        mov     bx, ax
        mov     ah, 40h         ; write the line
        mov     cx, msglen
        mov     dx, offset msg
        int     21h
        mov     ah, 3Eh
        int     21h
done:   mov     ax, 4C00h
        int     21h
fname   db      'COM.OUT', 0
msg     db      'com: written by a .COM made with exe2bin', 13, 10
msglen  equ     $ - msg
_TEXT   ends
        end     start
