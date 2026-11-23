#ifndef _SYS_MMAN_H
#define _SYS_MMAN_H
#include <sys/types.h>
_RS2_BEGIN
#define PROT_NONE 0
#define PROT_READ 1
#define PROT_WRITE 2
#define PROT_EXEC 4
#define MAP_SHARED 1
#define MAP_PRIVATE 2
#define MAP_FIXED 0x10
#define MAP_ANONYMOUS 0x20
#define MAP_ANON MAP_ANONYMOUS
#define MAP_NORESERVE 0
#define MAP_FAILED ((void *)-1)
#define MS_ASYNC 1
#define MS_INVALIDATE 2
#define MS_SYNC 4
#define MADV_NORMAL 0
#define MADV_RANDOM 1
#define MADV_SEQUENTIAL 2
#define MADV_WILLNEED 3
#define MADV_DONTNEED 4
#define POSIX_MADV_NORMAL 0
#define POSIX_MADV_RANDOM 1
#define POSIX_MADV_SEQUENTIAL 2
#define POSIX_MADV_WILLNEED 3
#define POSIX_MADV_DONTNEED 4
/* Memory above 4 GB. A file is read into it whole; a shared writable
 * mapping is written back by msync and munmap. munmap takes whole
 * mappings. */
void *mmap(void *, size_t, int, int, int, off_t);
int munmap(void *, size_t);
int msync(void *, size_t, int);
int mprotect(void *, size_t, int);
int madvise(void *, size_t, int);
int posix_madvise(void *, size_t, int);
_RS2_END
#endif
