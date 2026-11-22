#ifndef _MALLOC_H
#define _MALLOC_H
#include <stdlib.h>
_RS2_BEGIN
size_t malloc_usable_size(void *);
#define alloca(n) __builtin_alloca(n)
_RS2_END
#endif
