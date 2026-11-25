/* A 64-bit text program: the C library and the OS/2 API. */
#include <os2.h>
#include <stdio.h>

int main(int argc, char **argv) {
    ULONG version[2];
    DosQuerySysInfo(QSV_VERSION_MAJOR, QSV_VERSION_MINOR, version, sizeof version);
    printf("Hello from C, %d-bit, on OS/2 %u.%u (%d arguments)\n", (int)sizeof(void *) * 8, (unsigned)version[0], (unsigned)version[1], argc - 1);
    return 0;
}
