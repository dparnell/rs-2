#ifndef _CTYPE_H
#define _CTYPE_H
#include "_rs2libc.h"
_RS2_BEGIN
/* The "C" locale: ASCII only. */
int isalnum(int);
int isalpha(int);
int isblank(int);
int iscntrl(int);
int isdigit(int);
int isgraph(int);
int islower(int);
int isprint(int);
int ispunct(int);
int isspace(int);
int isupper(int);
int isxdigit(int);
int tolower(int);
int toupper(int);
int isascii(int);
int toascii(int);
_RS2_END
#endif
