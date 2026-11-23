#ifndef _DLFCN_H
#define _DLFCN_H
#include "_rs2libc.h"
_RS2_BEGIN
#define RTLD_LAZY 1
#define RTLD_NOW 2
#define RTLD_GLOBAL 0x100
#define RTLD_LOCAL 0
#define RTLD_DEFAULT ((void *)0)
/* DosLoadModule and DosQueryProcAddr; nothing is found without a handle. */
void *dlopen(const char *, int);
void *dlsym(void *__restrict, const char *__restrict);
int dlclose(void *);
char *dlerror(void);
_RS2_END
#endif
