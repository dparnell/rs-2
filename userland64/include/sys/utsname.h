#ifndef _SYS_UTSNAME_H
#define _SYS_UTSNAME_H
#include "../_rs2libc.h"
_RS2_BEGIN
struct utsname {
    char sysname[16], nodename[64], release[16], version[16], machine[16];
};
int uname(struct utsname *);
_RS2_END
#endif
