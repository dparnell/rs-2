/* rs2libc: general utilities (defined in rs2libc). */
#ifndef _RS2LIBC_STDLIB_H
#define _RS2LIBC_STDLIB_H

#include <stddef.h>

void *malloc(size_t n);
void *calloc(size_t n, size_t size);
void *realloc(void *p, size_t n);
void free(void *p);
int posix_memalign(void **out, size_t align, size_t size);
void abort(void) __attribute__((noreturn));
void exit(int code) __attribute__((noreturn));
void _exit(int code) __attribute__((noreturn));
int atexit(void (*f)(void));
char *getenv(const char *name);
int setenv(const char *name, const char *value, int overwrite);
int unsetenv(const char *name);

#endif
