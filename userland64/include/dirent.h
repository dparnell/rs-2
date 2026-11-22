#ifndef _DIRENT_H
#define _DIRENT_H
#include <sys/types.h>
_RS2_BEGIN
#define DT_UNKNOWN 0
#define DT_DIR 4
#define DT_REG 8
struct dirent {
    ino_t d_ino;
    unsigned char d_type;
    unsigned int d_attr;   /* the OS/2 attributes, FILE_* */
    off_t d_size;
    char d_name[256];
};
typedef struct _rs2_dir DIR;
DIR *opendir(const char *);
struct dirent *readdir(DIR *);
int closedir(DIR *);
void rewinddir(DIR *);
_RS2_END
#endif
