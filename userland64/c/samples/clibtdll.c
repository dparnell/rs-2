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
