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
int truncate(const char *, off_t);
/* OS/2 has no links: these fail with ENOSYS (EINVAL for readlink). */
int link(const char *, const char *);
int symlink(const char *, const char *);
ssize_t readlink(const char *__restrict, char *__restrict, size_t);
int getentropy(void *, size_t);
#define _SC_ARG_MAX 0
#define _SC_OPEN_MAX 4
#define _SC_PAGESIZE 30
#define _SC_GETPW_R_SIZE_MAX 70
#define _SC_NPROCESSORS_ONLN 84
long sysconf(int);
unsigned sleep(unsigned);
int usleep(unsigned long);
pid_t getpid(void);
pid_t getppid(void);
_RS2_NORETURN void _exit(int);
extern char *optarg _RS2_DATA(optarg);
extern int optind _RS2_DATA(optind);
extern int opterr _RS2_DATA(opterr);
extern int optopt _RS2_DATA(optopt);
extern char **environ _RS2_DATA(environ);
ssize_t pread(int, void *, size_t, off_t);
ssize_t pwrite(int, const void *, size_t, off_t);
int getpagesize(void);
uid_t getuid(void);
uid_t geteuid(void);
gid_t getgid(void);
gid_t getegid(void);
int gethostname(char *, size_t);
/* The program's full path. */
const char *getexecname(void);
unsigned alarm(unsigned);   /* never raises SIGALRM */
pid_t fork(void);           /* ENOSYS: use posix_spawn */
pid_t setsid(void);
int chown(const char *, uid_t, gid_t);
int fchown(int, uid_t, gid_t);
int execv(const char *, char *const[]);
int execve(const char *, char *const[], char *const[]);
int execvp(const char *, char *const[]);
int getopt(int, char *const[], const char *);
_RS2_END
#endif
