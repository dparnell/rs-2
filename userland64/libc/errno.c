/* OS/2 errors as errno values, and strerror. */
#include "libc.h"
#include <string.h>
#include <stdio.h>
#include <signal.h>

int _rs2_errno_of(APIRET rc) {
    switch (rc) {
    case 0: return 0;
    case 2: case 3: case 15: case 18: case 110: case 123: case 161: return ENOENT;
    case 4: case 113: return EMFILE;
    case 5: case 32: case 33: case 82: case 16: return EACCES;
    case 19: return EROFS;
    case 6: return EBADF;
    case 8: return ENOMEM;
    case 17: return EXDEV;
    case 39: case 112: return ENOSPC;
    case 80: return EEXIST;
    case 95: return EINTR;
    case 109: return EPIPE;
    case 128: return ECHILD;
    case 132: return ESPIPE;
    case 145: return ENOTEMPTY;
    case 191: case 193: return ENOEXEC;
    case 206: return ENAMETOOLONG;
    case 232: return EAGAIN;
    case 640: return ETIMEDOUT;
    case 1: case 12: case 87: return EINVAL;
    default: return EIO;
    }
}

int _rs2_oserr(APIRET rc) {
    errno = _rs2_errno_of(rc);
    return -1;
}

static const struct { int n; const char *s; } messages[] = {
    {0, "No error"},
    {EPERM, "Operation not permitted"},
    {ENOENT, "No such file or directory"},
    {ESRCH, "No such process"},
    {EINTR, "Interrupted call"},
    {EIO, "Input/output error"},
    {ENXIO, "No such device or address"},
    {E2BIG, "Argument list too long"},
    {ENOEXEC, "Not an executable program"},
    {EBADF, "Bad file handle"},
    {ECHILD, "No child processes"},
    {EAGAIN, "Resource temporarily unavailable"},
    {ENOMEM, "Not enough memory"},
    {EACCES, "Access denied"},
    {EFAULT, "Bad address"},
    {EBUSY, "Resource busy"},
    {EEXIST, "File exists"},
    {EXDEV, "Not the same drive"},
    {ENODEV, "No such device"},
    {ENOTDIR, "Not a directory"},
    {EISDIR, "Is a directory"},
    {EINVAL, "Invalid argument"},
    {ENFILE, "Too many open files in the system"},
    {EMFILE, "Too many open files"},
    {ENOTTY, "Not a terminal"},
    {EFBIG, "File too large"},
    {ENOSPC, "No space left on the drive"},
    {ESPIPE, "Cannot seek on this handle"},
    {EROFS, "Write-protected"},
    {EMLINK, "Too many links"},
    {EPIPE, "Broken pipe"},
    {EDOM, "Argument out of domain"},
    {ERANGE, "Result out of range"},
    {EDEADLK, "Deadlock avoided"},
    {ENAMETOOLONG, "File name too long"},
    {ENOLCK, "No locks available"},
    {ENOSYS, "Function not implemented"},
    {ENOTEMPTY, "Directory not empty"},
    {EILSEQ, "Invalid character sequence"},
    {EOVERFLOW, "Value too large"},
    {ETIMEDOUT, "Timed out"},
    {ENOTSUP, "Not supported"},
};

static const char *message(int n) {
    for (size_t i = 0; i < sizeof messages / sizeof messages[0]; i++)
        if (messages[i].n == n)
            return messages[i].s;
    return 0;
}

char *strerror(int n) {
    const char *m = message(n);
    if (m)
        return (char *)m;
    char *b = _rs2_self()->strerror;
    snprintf(b, sizeof _rs2_self()->strerror, "Unknown error %d", n);
    return b;
}

int strerror_r(int n, char *buf, size_t len) {
    const char *m = message(n);
    if (!m) {
        snprintf(buf, len, "Unknown error %d", n);
        return EINVAL;
    }
    if (strlen(m) >= len)
        return ERANGE;
    strcpy(buf, m);
    return 0;
}

void perror(const char *s) {
    const char *m = strerror(errno);
    if (s && *s)
        fprintf(stderr, "%s: %s\n", s, m);
    else
        fprintf(stderr, "%s\n", m);
}

char *strsignal(int sig) {
    switch (sig) {
    case SIGINT: return "Interrupt";
    case SIGILL: return "Illegal instruction";
    case SIGABRT: return "Aborted";
    case SIGFPE: return "Arithmetic exception";
    case SIGKILL: return "Killed";
    case SIGSEGV: return "Access violation";
    case SIGTERM: return "Terminated";
    case SIGBREAK: return "Break";
    default: return "Unknown signal";
    }
}
