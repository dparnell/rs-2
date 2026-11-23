/* Internals of rs2's 64-bit C library (docs/LIBC64.md). */
#ifndef RS2_LIBC_INTERNAL_H
#define RS2_LIBC_INTERNAL_H

#include <os2.h>
#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <time.h>

#define HIDDEN __attribute__((visibility("hidden")))

/* OS/2 error codes the library tells apart. */
#define ERROR_FILE_NOT_FOUND 2
#define ERROR_PATH_NOT_FOUND 3
#define ERROR_ACCESS_DENIED 5
#define ERROR_INVALID_HANDLE 6
#define ERROR_NOT_ENOUGH_MEMORY 8
#define ERROR_NO_MORE_FILES 18
#define ERROR_FILE_EXISTS 80
#define ERROR_OPEN_FAILED 110
#define ERROR_BROKEN_PIPE 109
#define ERROR_MORE_DATA 234

/* Each thread's own state, found through one dword of thread-local
 * memory (DosAllocThreadLocalMemory). */
struct _rs2_thread {
    int err;
    int sigdepth;
    char *strtok_next;
    struct tm tm;
    char asctime[32];
    char tmpnam[32];
    char strerror[48];
    /* The interrupted registers while a signal handler runs (signal.c). */
    unsigned long sigregs[20];
    /* pthread.c: the event __rs2_wait sleeps on, the pthread_t, keys,
     * thread_local destructors and variables. */
    HEV event;
    void *pthread;
    void **specific;
    void *dtors;
    void **tls;
    size_t tls_count;
};
HIDDEN struct _rs2_thread *_rs2_self(void);
HIDDEN void _rs2_thread_done(void);
HIDDEN void _rs2_thread_dtors(void);
HIDDEN void _rs2_thread_exit(void);

/* Sets errno from an OS/2 error; returns -1. */
HIDDEN int _rs2_oserr(APIRET rc);
HIDDEN int _rs2_errno_of(APIRET rc);

/* A lock: spins, then yields. Recursive locks belong to one thread. */
typedef struct {
    volatile int locked;
    struct _rs2_thread *owner;
    int count;
} _rs2_lock_t;
HIDDEN void _rs2_lock(_rs2_lock_t *);
HIDDEN void _rs2_unlock(_rs2_lock_t *);
HIDDEN void _rs2_spin_lock(volatile int *);
HIDDEN void _rs2_spin_unlock(volatile int *);

/* File descriptors (io.c). */
#define _RS2_FD_MAX 4096
#define _RS2_FD_TEXT 1
#define _RS2_FD_APPEND 2
#define _RS2_FD_EOF 4
#define _RS2_FD_PEEK 8
#define _RS2_FD_NOINHERIT 16
HIDDEN extern unsigned char _rs2_fdflags[_RS2_FD_MAX];
HIDDEN int _rs2_fdflag(int fd);
HIDDEN long _rs2_rawread(int fd, void *buf, size_t n);
HIDDEN long _rs2_rawwrite(int fd, const void *buf, size_t n);

/* A module's code and unwind tables (frames.c); link64.ld's symbols. */
struct _rs2_frames {
    const char *text, *text_end, *hdr, *hdr_end, *eh, *eh_end;
    struct _rs2_frames *next;
};
HIDDEN extern const char __text_start[], __text_end[], __eh_frame_hdr_start[], __eh_frame_hdr_end[], __eh_frame_start[],
    __eh_frame_end[];
#define RS2_FRAMES {__text_start, __text_end, __eh_frame_hdr_start, __eh_frame_hdr_end, __eh_frame_start, __eh_frame_end, 0}
void __rs2_add_frames(struct _rs2_frames *);
void __rs2_remove_frames(struct _rs2_frames *);
int __rs2_find_frames(const void *pc, struct _rs2_frames *out);

/* Start-up (start.c, crt1.c). */
struct _rs2_startup {
    ULONG hmod;
    PCSZ env, cmdline;
    int (*main)(int, char **, char **);
    void (**init_start)(void), (**init_end)(void);
    void (**fini_start)(void), (**fini_end)(void);
};
__attribute__((noreturn)) void __rs2_start(const struct _rs2_startup *);
HIDDEN extern ULONG _rs2_start_ms;
HIDDEN extern char **_rs2_argv;
HIDDEN extern int _rs2_environ_owned;
extern char **environ;
HIDDEN void _rs2_stdio_exit(void);
HIDDEN char *_rs2_env_block(void);
HIDDEN void _rs2_signal_init(void);

/* Formatting (printf.c, scanf.c). */
typedef void (*_rs2_out_fn)(void *ctx, const char *s, size_t n);
HIDDEN int _rs2_vformat(_rs2_out_fn out, void *ctx, const char *fmt, __builtin_va_list ap);
typedef struct {
    int (*get)(void *ctx);
    void (*unget)(int c, void *ctx);
    void *ctx;
} _rs2_in_t;
HIDDEN int _rs2_vscan(_rs2_in_t *in, const char *fmt, __builtin_va_list ap);

/* Time (time.c): seconds between local time and UTC (UTC = local + off). */
HIDDEN long _rs2_tz_offset(time_t utc, int *isdst);
HIDDEN long long _rs2_days_from_civil(long long y, unsigned m, unsigned d);

/* Programs: the argument string "prog\0args\0\0" for DosExecPgm (process.c). */
HIDDEN char *_rs2_build_args(const char *prog, char *const argv[]);

#endif
