#ifndef _STRINGS_H
#define _STRINGS_H
#include "_rs2libc.h"
_RS2_BEGIN
int strcasecmp(const char *, const char *);
int strncasecmp(const char *, const char *, size_t);
int ffs(int);
int bcmp(const void *, const void *, size_t);
_RS2_END
#endif
