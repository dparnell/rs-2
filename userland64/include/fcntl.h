#ifndef _FCNTL_H
#define _FCNTL_H
#include <sys/types.h>
_RS2_BEGIN
#define O_RDONLY 0x0000
#define O_WRONLY 0x0001
#define O_RDWR 0x0002
#define O_ACCMODE 0x0003
#define O_APPEND 0x0008
#define O_CREAT 0x0100
#define O_TRUNC 0x0200
#define O_EXCL 0x0400
#define O_NOINHERIT 0x0800
#define O_TEXT 0x4000
#define O_BINARY 0x8000
#define O_CLOEXEC O_NOINHERIT
#define O_NOCTTY 0
#define O_NONBLOCK 0
#define F_GETFD 1
#define F_SETFD 2
#define F_GETFL 3
#define F_SETFL 4
#define F_GETLK 5
#define F_SETLK 6
#define F_SETLKW 7
#define F_RDLCK 0
#define F_WRLCK 1
#define F_UNLCK 2
#define FD_CLOEXEC 1
#define AT_FDCWD (-100)
/* Locks are OS/2's (DosSetFileLocks): they keep other processes from
 * reading and writing what is locked. l_whence is SEEK_SET. */
struct flock {
    short l_type, l_whence;
    off_t l_start, l_len;
    pid_t l_pid;
};
int open(const char *, int, ...);
int creat(const char *, mode_t);
int fcntl(int, int, ...);
_RS2_END
#endif
