#ifndef _PWD_H
#define _PWD_H
#include <sys/types.h>
_RS2_BEGIN
struct passwd {
    char *pw_name, *pw_passwd, *pw_gecos, *pw_dir, *pw_shell;
    uid_t pw_uid;
    gid_t pw_gid;
};
/* There are no users: not found. */
struct passwd *getpwuid(uid_t);
struct passwd *getpwnam(const char *);
_RS2_END
#endif
