/* CLIBTDLL.DLL: a C DLL for CLIBTEST (docs/LIBC64.md). With LIBC64.DLL
 * the program and this DLL share one C library: memory malloc'd here may
 * be freed there, both write to the same stdout buffer, and errno is the
 * thread's own in both. */
#include <os2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <errno.h>

RS2_EXPORT char *dll_upper(const char *s) {
    char *d = strdup(s);
    for (char *p = d; p && *p; p++)
        *p = (char)toupper((unsigned char)*p);
    return d;
}

RS2_EXPORT void dll_print(const char *s) {
    printf("%s", s);   /* no newline: it stays in the shared buffer */
}

RS2_EXPORT void dll_set_errno(int e) {
    errno = e;
}

/* Constructors and destructors (crtdll.o runs them). The destructor
 * leaves a file, MARK (named by the build), so the program can see it
 * ran: at DosFreeModule, or at the end of a process. */
#ifndef MARK
#define MARK "CLIBTDLL.FIN"
#endif
static int ctor_count;
static char *ctor_alloc;

__attribute__((constructor)) static void ctor(void) {
    ctor_count++;
    ctor_alloc = malloc(16);   /* the C library works in a constructor */
    if (ctor_alloc)
        strcpy(ctor_alloc, "constructed");
}

__attribute__((destructor)) static void dtor(void) {
    FILE *f = fopen(MARK, "w");
    if (f) {
        fputs(ctor_alloc ? ctor_alloc : "?", f);
        fclose(f);
    }
    free(ctor_alloc);
    ctor_alloc = 0;
}

RS2_EXPORT int dll_ctor_count(void) {
    return ctor_count;
}
