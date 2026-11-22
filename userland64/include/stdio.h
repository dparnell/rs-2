#ifndef _STDIO_H
#define _STDIO_H
#include "_rs2libc.h"
#include <stdarg.h>
_RS2_BEGIN
typedef struct _rs2_file FILE;
typedef long long fpos_t;
#ifndef _OFF_T_DEFINED
#define _OFF_T_DEFINED
typedef long long off_t;
#endif
#ifndef _SSIZE_T_DEFINED
#define _SSIZE_T_DEFINED
typedef long ssize_t;
#endif
#define EOF (-1)
#define BUFSIZ 4096
#define FOPEN_MAX 1000
#define FILENAME_MAX 260
#define L_tmpnam 32
#define TMP_MAX 1000000
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2
extern FILE *const stdin;
extern FILE *const stdout;
extern FILE *const stderr;
#define stdin stdin
#define stdout stdout
#define stderr stderr

FILE *fopen(const char *__restrict, const char *__restrict);
FILE *freopen(const char *__restrict, const char *__restrict, FILE *__restrict);
FILE *fdopen(int, const char *);
int fclose(FILE *);
int fflush(FILE *);
int setvbuf(FILE *__restrict, char *__restrict, int, size_t);
void setbuf(FILE *__restrict, char *__restrict);
int fileno(FILE *);

size_t fread(void *__restrict, size_t, size_t, FILE *__restrict);
size_t fwrite(const void *__restrict, size_t, size_t, FILE *__restrict);
int fgetc(FILE *);
int getc(FILE *);
int getchar(void);
int ungetc(int, FILE *);
char *fgets(char *__restrict, int, FILE *__restrict);
ssize_t getline(char **__restrict, size_t *__restrict, FILE *__restrict);
ssize_t getdelim(char **__restrict, size_t *__restrict, int, FILE *__restrict);
int fputc(int, FILE *);
int putc(int, FILE *);
int putchar(int);
int fputs(const char *__restrict, FILE *__restrict);
int puts(const char *);

int fseek(FILE *, long, int);
int fseeko(FILE *, off_t, int);
long ftell(FILE *);
off_t ftello(FILE *);
void rewind(FILE *);
int fgetpos(FILE *__restrict, fpos_t *__restrict);
int fsetpos(FILE *, const fpos_t *);
void clearerr(FILE *);
int feof(FILE *);
int ferror(FILE *);
void perror(const char *);

int printf(const char *__restrict, ...) _RS2_PRINTF(1, 2);
int fprintf(FILE *__restrict, const char *__restrict, ...) _RS2_PRINTF(2, 3);
int sprintf(char *__restrict, const char *__restrict, ...) _RS2_PRINTF(2, 3);
int snprintf(char *__restrict, size_t, const char *__restrict, ...) _RS2_PRINTF(3, 4);
int dprintf(int, const char *__restrict, ...) _RS2_PRINTF(2, 3);
int asprintf(char **, const char *, ...) _RS2_PRINTF(2, 3);
int vprintf(const char *__restrict, va_list) _RS2_PRINTF(1, 0);
int vfprintf(FILE *__restrict, const char *__restrict, va_list) _RS2_PRINTF(2, 0);
int vsprintf(char *__restrict, const char *__restrict, va_list) _RS2_PRINTF(2, 0);
int vsnprintf(char *__restrict, size_t, const char *__restrict, va_list) _RS2_PRINTF(3, 0);
int vdprintf(int, const char *__restrict, va_list) _RS2_PRINTF(2, 0);
int vasprintf(char **, const char *, va_list) _RS2_PRINTF(2, 0);

int scanf(const char *__restrict, ...) _RS2_SCANF(1, 2);
int fscanf(FILE *__restrict, const char *__restrict, ...) _RS2_SCANF(2, 3);
int sscanf(const char *__restrict, const char *__restrict, ...) _RS2_SCANF(2, 3);
int vscanf(const char *__restrict, va_list) _RS2_SCANF(1, 0);
int vfscanf(FILE *__restrict, const char *__restrict, va_list) _RS2_SCANF(2, 0);
int vsscanf(const char *__restrict, const char *__restrict, va_list) _RS2_SCANF(2, 0);

int remove(const char *);
int rename(const char *, const char *);
FILE *tmpfile(void);
char *tmpnam(char *);
FILE *popen(const char *, const char *);
int pclose(FILE *);
void flockfile(FILE *);
void funlockfile(FILE *);
int ftrylockfile(FILE *);
int getc_unlocked(FILE *);
int getchar_unlocked(void);
int putc_unlocked(int, FILE *);
int putchar_unlocked(int);
_RS2_END
#endif
