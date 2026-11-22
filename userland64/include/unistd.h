#ifndef _UNISTD_H
#define _UNISTD_H
#include <sys/types.h>
_RS2_BEGIN
#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4
#ifndef SEEK_SET
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#endif
ssize_t read(int, void *, size_t);
ssize_t write(int, const void *, size_t);
int close(int);
off_t lseek(int, off_t, int);
int dup(int);
int dup2(int, int);
int pipe(int[2]);
int isatty(int);
int access(const char *, int);
int unlink(const char *);
int rmdir(const char *);
int chdir(const char *);
char *getcwd(char *, size_t);
int ftruncate(int, off_t);
int fsync(int);
unsigned sleep(unsigned);
int usleep(unsigned long);
pid_t getpid(void);
pid_t getppid(void);
_RS2_NORETURN void _exit(int);
extern char *optarg;
extern int optind, opterr, optopt;
int getopt(int, char *const[], const char *);
_RS2_END
#endif
