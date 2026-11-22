#ifndef _DIRECT_H
#define _DIRECT_H
#include <unistd.h>
#include <sys/stat.h>
_RS2_BEGIN
/* Drives: 1 = A:, 2 = B:, ... */
int _getdrive(void);
int _chdrive(int);
char *_getdcwd(int, char *, size_t);
_RS2_END
#endif
