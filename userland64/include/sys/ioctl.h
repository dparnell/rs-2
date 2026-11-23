#ifndef _SYS_IOCTL_H
#define _SYS_IOCTL_H
#include <sys/types.h>
_RS2_BEGIN
struct winsize {
    unsigned short ws_row, ws_col, ws_xpixel, ws_ypixel;
};
#define TIOCGWINSZ 0x5413
int ioctl(int, unsigned long, ...);
_RS2_END
#endif
