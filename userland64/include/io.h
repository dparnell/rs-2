/* The handle-level I/O of OS/2 and DOS compilers. File descriptors are
 * OS/2 file handles. */
#ifndef _IO_H
#define _IO_H
#include <unistd.h>
#include <fcntl.h>
#include <sys/stat.h>
_RS2_BEGIN
int setmode(int, int);
off_t tell(int);
off_t filelength(int);
int eof(int);
int chsize(int, off_t);
_RS2_END
#endif
