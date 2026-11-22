/* setjmp and longjmp: rbx, rbp, r12-r15, rsp, the return address, the
 * SSE control/status word and the x87 control word. Signal trampolines
 * for signal.c. */
    .text
    .globl setjmp, _setjmp, longjmp, _longjmp
    .type setjmp,@function
setjmp:
_setjmp:
    mov %rbx, 0(%rdi)
    mov %rbp, 8(%rdi)
    mov %r12, 16(%rdi)
    mov %r13, 24(%rdi)
    mov %r14, 32(%rdi)
    mov %r15, 40(%rdi)
    lea 8(%rsp), %rdx
    mov %rdx, 48(%rdi)
    mov (%rsp), %rdx
    mov %rdx, 56(%rdi)
    stmxcsr 64(%rdi)
    fnstcw 68(%rdi)
    xor %eax, %eax
    ret

    .type longjmp,@function
longjmp:
_longjmp:
    mov %esi, %eax
    test %eax, %eax
    jnz 1f
    inc %eax
1:  mov 0(%rdi), %rbx
    mov 8(%rdi), %rbp
    mov 16(%rdi), %r12
    mov 24(%rdi), %r13
    mov 32(%rdi), %r14
    mov 40(%rdi), %r15
    ldmxcsr 64(%rdi)
    fldcw 68(%rdi)
    mov 48(%rdi), %rsp
    jmp *56(%rdi)

/* _rs2_sigtramp: entered (by an exception handler's changed context) with
 * rdi = the signal, rsi = the handler, rdx = the saved registers (rax rbx
 * rcx rdx rsi rdi rbp rsp r8-r15 rip rflags, 18 quadwords). Copies them
 * to its stack, calls the handler, then resumes the interrupted code with
 * IRETQ (same privilege: it loads RSP without touching the stack below
 * it, where the interrupted code may keep data). */
    .globl _rs2_sigtramp
    .type _rs2_sigtramp,@function
_rs2_sigtramp:
    and $-16, %rsp
    sub $160, %rsp
    mov %rsi, %rax
    mov %rdx, %rsi
    mov %rsp, %r12
    xor %ecx, %ecx
2:  mov (%rsi,%rcx,8), %r8
    mov %r8, (%r12,%rcx,8)
    inc %ecx
    cmp $18, %ecx
    jne 2b
    sub $512, %rsp
    fxsave64 (%rsp)
    call *%rax
    fxrstor64 (%rsp)
    mov %r12, %r15
    /* The IRETQ frame: rip, cs, rflags, rsp, ss. */
    mov %ss, %eax
    push %rax
    push 56(%r15)
    push 136(%r15)
    mov %cs, %eax
    push %rax
    push 128(%r15)
    mov 0(%r15), %rax
    mov 8(%r15), %rbx
    mov 16(%r15), %rcx
    mov 24(%r15), %rdx
    mov 32(%r15), %rsi
    mov 40(%r15), %rdi
    mov 48(%r15), %rbp
    mov 64(%r15), %r8
    mov 72(%r15), %r9
    mov 80(%r15), %r10
    mov 88(%r15), %r11
    mov 96(%r15), %r12
    mov 104(%r15), %r13
    mov 112(%r15), %r14
    mov 120(%r15), %r15
    iretq

    .section .note.GNU-stack,"",@progbits
