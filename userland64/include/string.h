#ifndef _STRING_H
#define _STRING_H
#include "_rs2libc.h"
#include <strings.h>
_RS2_BEGIN
void *memcpy(void *__restrict, const void *__restrict, size_t);
void *memmove(void *, const void *, size_t);
void *memset(void *, int, size_t);
int memcmp(const void *, const void *, size_t);
void *memchr(const void *, int, size_t);
void *memrchr(const void *, int, size_t);
void *memccpy(void *__restrict, const void *__restrict, int, size_t);
char *strcpy(char *__restrict, const char *__restrict);
char *strncpy(char *__restrict, const char *__restrict, size_t);
char *stpcpy(char *__restrict, const char *__restrict);
char *strcat(char *__restrict, const char *__restrict);
char *strncat(char *__restrict, const char *__restrict, size_t);
size_t strlcpy(char *__restrict, const char *__restrict, size_t);
size_t strlcat(char *__restrict, const char *__restrict, size_t);
int strcmp(const char *, const char *);
int strncmp(const char *, const char *, size_t);
int strcoll(const char *, const char *);
size_t strxfrm(char *__restrict, const char *__restrict, size_t);
char *strchr(const char *, int);
char *strrchr(const char *, int);
size_t strcspn(const char *, const char *);
size_t strspn(const char *, const char *);
char *strpbrk(const char *, const char *);
char *strstr(const char *, const char *);
char *strtok(char *__restrict, const char *__restrict);
char *strtok_r(char *__restrict, const char *__restrict, char **__restrict);
size_t strlen(const char *);
size_t strnlen(const char *, size_t);
char *strerror(int);
int strerror_r(int, char *, size_t);
char *strdup(const char *);
char *strndup(const char *, size_t);
char *strsep(char **, const char *);
char *strsignal(int);
void *memmem(const void *, size_t, const void *, size_t);
/* Common in OS/2 and DOS compilers. */
int stricmp(const char *, const char *);
int strnicmp(const char *, const char *, size_t);
char *strupr(char *);
char *strlwr(char *);
char *strrev(char *);
_RS2_END
#endif
