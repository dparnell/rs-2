#ifndef _SYS_STATVFS_H
#define _SYS_STATVFS_H
#include <sys/types.h>
_RS2_BEGIN
typedef unsigned long fsblkcnt_t;
typedef unsigned long fsfilcnt_t;
struct statvfs {
    unsigned long f_bsize, f_frsize;
    fsblkcnt_t f_blocks, f_bfree, f_bavail;
    fsfilcnt_t f_files, f_ffree, f_favail;
    unsigned long f_fsid, f_flag, f_namemax;
};
#define ST_RDONLY 1
int fstatvfs(int, struct statvfs *);
int statvfs(const char *__restrict, struct statvfs *__restrict);
_RS2_END
#endif
