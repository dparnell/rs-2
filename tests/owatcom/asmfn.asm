; wasm: 32-bit flat functions called from C (Watcom register calling
; convention: first argument in EAX, second in EDX; result in EAX)
.386
.model flat
.code
        public  AsmMulAdd_
AsmMulAdd_ proc near
        imul    eax, edx        ; a * b
        add     eax, 7
        ret
AsmMulAdd_ endp

        public  AsmCountBits_
AsmCountBits_ proc near
        push    ecx             ; the convention: only EAX may change
        push    edx
        xor     ecx, ecx
again:  test    eax, eax
        jz      done
        mov     edx, eax
        dec     edx
        and     eax, edx
        inc     ecx
        jmp     again
done:   mov     eax, ecx
        pop     edx
        pop     ecx
        ret
AsmCountBits_ endp
        end
