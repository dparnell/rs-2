/* Handle-level I/O. A file descriptor is the OS/2 file handle; handles
 * opened in text mode turn CR LF into LF when read (Ctrl+Z ends the
 * file) and LF into CR LF when written. */
#include "libc.h"
#include <fcntl.h>
#include <io.h>
#include <direct.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <dirent.h>
#include <sys/stat.h>

/* stdin, stdout and stderr start in text mode. */
unsigned char _rs2_fdflags[_RS2_FD_MAX] = {_RS2_FD_TEXT, _RS2_FD_TEXT, _RS2_FD_TEXT};
static unsigned char peek[_RS2_FD_MAX];

int _rs2_fdflag(int fd) {
    return fd >= 0 && fd < _RS2_FD_MAX ? _rs2_fdflags[fd] : 0;
}

static void setflags(int fd, int f) {
    if (fd >= 0 && fd < _RS2_FD_MAX)
        _rs2_fdflags[fd] = (unsigned char)f;
}

long _rs2_rawread(int fd, void *buf, size_t n) {
    ULONG got = 0;
    if (n > 0x7fffffff)
        n = 0x7fffffff;
    APIRET rc = DosRead((HFILE)fd, buf, (ULONG)n, &got);
    if (rc == ERROR_BROKEN_PIPE || rc == ERROR_MORE_DATA)
        return got;
    if (rc)
        return _rs2_oserr(rc);
    return got;
}

long _rs2_rawwrite(int fd, const void *buf, size_t n) {
    if (_rs2_fdflag(fd) & _RS2_FD_APPEND) {
        LONGLONG pos;
        DosSetFilePtrL((HFILE)fd, 0, FILE_END, &pos);
    }
    size_t done = 0;
    while (done < n) {
        ULONG chunk = n - done > 0x40000000 ? 0x40000000 : (ULONG)(n - done), put = 0;
        APIRET rc = DosWrite((HFILE)fd, (const char *)buf + done, chunk, &put);
        if (rc) {
            if (done)
                return done;
            return _rs2_oserr(rc);
        }
        done += put;
        if (put < chunk) {
            if (!done) {
                errno = ENOSPC;
                return -1;
            }
            break;
        }
    }
    return done;
}

ssize_t read(int fd, void *buf, size_t n) {
    int fl = _rs2_fdflag(fd);
    if (!(fl & _RS2_FD_TEXT))
        return _rs2_rawread(fd, buf, n);
    if ((fl & _RS2_FD_EOF) || n == 0)
        return 0;
    unsigned char *b = buf;
    for (;;) {
        size_t got = 0;
        if (fl & _RS2_FD_PEEK) {
            b[0] = peek[fd];
            got = 1;
            fl &= ~_RS2_FD_PEEK;
            setflags(fd, fl);
        }
        if (got < n) {
            long r = _rs2_rawread(fd, b + got, n - got);
            if (r < 0)
                return got ? (ssize_t)got : -1;
            got += r;
        }
        if (got == 0)
            return 0;
        size_t out = 0;
        for (size_t i = 0; i < got; i++) {
            unsigned char c = b[i];
            if (c == 0x1a) {
                setflags(fd, _rs2_fdflag(fd) | _RS2_FD_EOF);
                break;
            }
            if (c == '\r') {
                if (i + 1 < got) {
                    if (b[i + 1] == '\n')
                        continue;
                } else {
                    unsigned char nx;
                    if (_rs2_rawread(fd, &nx, 1) == 1) {
                        if (nx == '\n') {
                            b[out++] = '\n';
                            break;
                        }
                        peek[fd] = nx;
                        setflags(fd, _rs2_fdflag(fd) | _RS2_FD_PEEK);
                    }
                }
            }
            b[out++] = c;
        }
        if (out || (_rs2_fdflag(fd) & _RS2_FD_EOF))
            return out;
        fl = _rs2_fdflag(fd);
    }
}

ssize_t write(int fd, const void *buf, size_t n) {
    if (!(_rs2_fdflag(fd) & _RS2_FD_TEXT))
        return _rs2_rawwrite(fd, buf, n);
    const char *s = buf;
    char tmp[1024];
    size_t i = 0;
    while (i < n) {
        size_t t = 0, start = i;
        while (i < n && t < sizeof tmp - 1) {
            if (s[i] == '\n')
                tmp[t++] = '\r';
            tmp[t++] = s[i++];
        }
        long r = _rs2_rawwrite(fd, tmp, t);
        if (r < 0)
            return start ? (ssize_t)start : -1;
        if ((size_t)r < t)
            return start + r;
    }
    return n;
}

int open(const char *path, int flags, ...) {
    mode_t mode = 0666;
    if (flags & O_CREAT) {
        va_list ap;
        va_start(ap, flags);
        mode = va_arg(ap, int);
        va_end(ap);
    }
    ULONG act, oflag, omode;
    if (flags & O_CREAT) {
        if (flags & O_EXCL)
            oflag = OPEN_ACTION_FAIL_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW;
        else if (flags & O_TRUNC)
            oflag = OPEN_ACTION_REPLACE_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW;
        else
            oflag = OPEN_ACTION_OPEN_IF_EXISTS | OPEN_ACTION_CREATE_IF_NEW;
    } else {
        oflag = (flags & O_TRUNC ? OPEN_ACTION_REPLACE_IF_EXISTS : OPEN_ACTION_OPEN_IF_EXISTS) | OPEN_ACTION_FAIL_IF_NEW;
    }
    switch (flags & O_ACCMODE) {
    case O_WRONLY: omode = OPEN_ACCESS_WRITEONLY; break;
    case O_RDWR: omode = OPEN_ACCESS_READWRITE; break;
    default: omode = OPEN_ACCESS_READONLY; break;
    }
    omode |= OPEN_SHARE_DENYNONE | OPEN_FLAGS_FAIL_ON_ERROR;
    if (flags & O_NOINHERIT)
        omode |= OPEN_FLAGS_NOINHERIT;
    ULONG attr = (flags & O_CREAT) && !(mode & S_IWUSR) ? FILE_READONLY : FILE_NORMAL;
    HFILE h = 0;
    APIRET rc = DosOpenL((PCSZ)path, &h, &act, 0, attr, oflag, omode, 0);
    if (rc) {
        if (rc == ERROR_ACCESS_DENIED) {
            FILESTATUS3 st;
            if (DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st) == 0 && (st.attrFile & FILE_DIRECTORY)) {
                errno = EISDIR;
                return -1;
            }
        }
        if (rc == ERROR_FILE_EXISTS || (rc == ERROR_ACCESS_DENIED && (flags & O_EXCL) && access(path, F_OK) == 0)) {
            errno = EEXIST;
            return -1;
        }
        /* ERROR_OPEN_FAILED: the open action said to fail, because the
         * file exists (O_EXCL) or because it does not. */
        if (rc == ERROR_OPEN_FAILED) {
            errno = (flags & (O_CREAT | O_EXCL)) == (O_CREAT | O_EXCL) ? EEXIST : ENOENT;
            return -1;
        }
        return _rs2_oserr(rc);
    }
    int f = 0;
    if (flags & O_TEXT)
        f |= _RS2_FD_TEXT;
    if (flags & O_APPEND)
        f |= _RS2_FD_APPEND;
    if (flags & O_NOINHERIT)
        f |= _RS2_FD_NOINHERIT;
    setflags((int)h, f);
    return (int)h;
}

int creat(const char *path, mode_t mode) {
    return open(path, O_WRONLY | O_CREAT | O_TRUNC, mode);
}

int close(int fd) {
    APIRET rc = DosClose((HFILE)fd);
    setflags(fd, 0);
    return rc ? _rs2_oserr(rc) : 0;
}

off_t lseek(int fd, off_t off, int whence) {
    LONGLONG pos = 0;
    APIRET rc = DosSetFilePtrL((HFILE)fd, off, (ULONG)whence, &pos);
    if (rc)
        return _rs2_oserr(rc);
    setflags(fd, _rs2_fdflag(fd) & ~(_RS2_FD_EOF | _RS2_FD_PEEK));
    return pos;
}

off_t tell(int fd) {
    LONGLONG pos = 0;
    APIRET rc = DosSetFilePtrL((HFILE)fd, 0, FILE_CURRENT, &pos);
    return rc ? _rs2_oserr(rc) : pos;
}

off_t filelength(int fd) {
    LONGLONG cur, end;
    if (DosSetFilePtrL((HFILE)fd, 0, FILE_CURRENT, &cur))
        return -1;
    APIRET rc = DosSetFilePtrL((HFILE)fd, 0, FILE_END, &end);
    DosSetFilePtrL((HFILE)fd, cur, FILE_BEGIN, &cur);
    return rc ? _rs2_oserr(rc) : end;
}

int eof(int fd) {
    off_t len = filelength(fd);
    return len < 0 ? -1 : tell(fd) >= len;
}

int chsize(int fd, off_t size) {
    APIRET rc = DosSetFileSizeL((HFILE)fd, size);
    return rc ? _rs2_oserr(rc) : 0;
}

int ftruncate(int fd, off_t size) {
    return chsize(fd, size);
}

int fsync(int fd) {
    APIRET rc = DosResetBuffer((HFILE)fd);
    return rc ? _rs2_oserr(rc) : 0;
}

int setmode(int fd, int mode) {
    int old = _rs2_fdflag(fd) & _RS2_FD_TEXT ? O_TEXT : O_BINARY;
    int f = _rs2_fdflag(fd) & ~_RS2_FD_TEXT;
    if (mode & O_TEXT)
        f |= _RS2_FD_TEXT;
    setflags(fd, f);
    return old;
}

int dup(int fd) {
    HFILE h = (HFILE)-1;
    APIRET rc = DosDupHandle((HFILE)fd, &h);
    if (rc)
        return _rs2_oserr(rc);
    setflags((int)h, _rs2_fdflag(fd) & (_RS2_FD_TEXT | _RS2_FD_APPEND));
    return (int)h;
}

int dup2(int fd, int to) {
    if (fd == to)
        return to;
    HFILE h = (HFILE)to;
    APIRET rc = DosDupHandle((HFILE)fd, &h);
    if (rc)
        return _rs2_oserr(rc);
    setflags(to, _rs2_fdflag(fd) & (_RS2_FD_TEXT | _RS2_FD_APPEND));
    return to;
}

int pipe(int fds[2]) {
    HFILE r, w;
    APIRET rc = DosCreatePipe(&r, &w, 65536);
    if (rc)
        return _rs2_oserr(rc);
    fds[0] = (int)r;
    fds[1] = (int)w;
    setflags(fds[0], 0);
    setflags(fds[1], 0);
    return 0;
}

int fcntl(int fd, int cmd, ...) {
    ULONG st;
    switch (cmd) {
    case F_GETFD:
        if (DosQueryFHState((HFILE)fd, &st))
            return _rs2_oserr(ERROR_INVALID_HANDLE);
        return st & OPEN_FLAGS_NOINHERIT ? FD_CLOEXEC : 0;
    case F_SETFD: {
        va_list ap;
        va_start(ap, cmd);
        int v = va_arg(ap, int);
        va_end(ap);
        APIRET rc = DosQueryFHState((HFILE)fd, &st);
        if (rc)
            return _rs2_oserr(rc);
        st &= 0x7080;   /* the bits DosSetFHState takes */
        st = v & FD_CLOEXEC ? st | OPEN_FLAGS_NOINHERIT : st & ~OPEN_FLAGS_NOINHERIT;
        rc = DosSetFHState((HFILE)fd, st);
        return rc ? _rs2_oserr(rc) : 0;
    }
    case F_GETFL: {
        APIRET rc = DosQueryFHState((HFILE)fd, &st);
        if (rc)
            return _rs2_oserr(rc);
        int f = st & 3;
        if (_rs2_fdflag(fd) & _RS2_FD_APPEND)
            f |= O_APPEND;
        f |= _rs2_fdflag(fd) & _RS2_FD_TEXT ? O_TEXT : O_BINARY;
        return f;
    }
    }
    errno = EINVAL;
    return -1;
}

int isatty(int fd) {
    ULONG type, attr;
    if (DosQueryHType((HFILE)fd, &type, &attr)) {
        errno = EBADF;
        return 0;
    }
    if ((type & 0xff) == HANDTYPE_DEVICE)
        return 1;
    errno = ENOTTY;
    return 0;
}

int access(const char *path, int mode) {
    FILESTATUS3 st;
    APIRET rc = DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st);
    if (rc) {
        /* The root of a drive has no directory entry. */
        if (path[0] && path[1] == ':' && (!path[2] || ((path[2] == '\\' || path[2] == '/') && !path[3]))) {
            return 0;
        }
        return _rs2_oserr(rc);
    }
    if ((mode & W_OK) && (st.attrFile & FILE_READONLY)) {
        errno = EACCES;
        return -1;
    }
    return 0;
}

int unlink(const char *path) {
    APIRET rc = DosDelete((PCSZ)path);
    if (rc == ERROR_ACCESS_DENIED) {
        FILESTATUS3 st;
        if (DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st) == 0 && (st.attrFile & FILE_DIRECTORY)) {
            errno = EISDIR;
            return -1;
        }
    }
    return rc ? _rs2_oserr(rc) : 0;
}

int remove(const char *path) {
    FILESTATUS3 st;
    if (DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st) == 0 && (st.attrFile & FILE_DIRECTORY))
        return rmdir(path);
    return unlink(path);
}

int rename(const char *from, const char *to) {
    APIRET rc = DosMove((PCSZ)from, (PCSZ)to);
    if (rc == ERROR_ACCESS_DENIED || rc == ERROR_FILE_EXISTS) {
        /* C lets rename replace a file; OS/2 does not. */
        FILESTATUS3 st;
        if (DosQueryPathInfo((PCSZ)to, FIL_STANDARD, &st, sizeof st) == 0 && !(st.attrFile & FILE_DIRECTORY) &&
            DosDelete((PCSZ)to) == 0)
            rc = DosMove((PCSZ)from, (PCSZ)to);
    }
    return rc ? _rs2_oserr(rc) : 0;
}

int mkdir(const char *path, mode_t mode) {
    (void)mode;
    APIRET rc = DosCreateDir((PCSZ)path, 0);
    if (rc == ERROR_ACCESS_DENIED && access(path, F_OK) == 0) {
        errno = EEXIST;
        return -1;
    }
    return rc ? _rs2_oserr(rc) : 0;
}

int rmdir(const char *path) {
    APIRET rc = DosDeleteDir((PCSZ)path);
    return rc ? _rs2_oserr(rc) : 0;
}

int _getdrive(void) {
    ULONG d = 0, map;
    DosQueryCurrentDisk(&d, &map);
    return (int)d;
}

int _chdrive(int d) {
    APIRET rc = DosSetDefaultDisk((ULONG)d);
    return rc ? _rs2_oserr(rc) : 0;
}

int chdir(const char *path) {
    if (path[0] && path[1] == ':') {
        int d = (path[0] | 0x20) - 'a' + 1;
        if (_chdrive(d))
            return -1;
        if (!path[2])
            return 0;
    }
    APIRET rc = DosSetCurrentDir((PCSZ)path);
    return rc ? _rs2_oserr(rc) : 0;
}

char *_getdcwd(int drive, char *buf, size_t size) {
    char tmp[PATH_MAX + 4];
    if (drive == 0)
        drive = _getdrive();
    tmp[0] = (char)('A' + drive - 1);
    tmp[1] = ':';
    tmp[2] = '\\';
    ULONG len = sizeof tmp - 3;
    APIRET rc = DosQueryCurrentDir((ULONG)drive, (PBYTE)tmp + 3, &len);
    if (rc) {
        _rs2_oserr(rc);
        return 0;
    }
    size_t n = strlen(tmp) + 1;
    if (!buf) {
        if (size < n)
            size = n;
        buf = malloc(size);
        if (!buf)
            return 0;
    } else if (size < n) {
        errno = ERANGE;
        return 0;
    }
    memcpy(buf, tmp, n);
    return buf;
}

char *getcwd(char *buf, size_t size) {
    return _getdcwd(0, buf, size);
}

/* An FDATE and FTIME (local time) as a time_t. */
static time_t file_time(FDATE d, FTIME t) {
    if (!d.date)
        return 0;
    long long days = _rs2_days_from_civil(1980 + (d.date >> 9), (d.date >> 5) & 15, d.date & 31);
    time_t local = days * 86400 + (t.time >> 11) * 3600 + ((t.time >> 5) & 63) * 60 + (t.time & 31) * 2;
    int dst;
    return local + _rs2_tz_offset(local, &dst);
}

static void fill_stat(struct stat *s, const FILESTATUS3 *st, int drive) {
    memset(s, 0, sizeof *s);
    s->st_dev = s->st_rdev = drive;
    s->st_nlink = 1;
    s->st_attr = st->attrFile;
    s->st_mode = st->attrFile & FILE_DIRECTORY ? S_IFDIR | 0755 : S_IFREG | 0644;
    if (st->attrFile & FILE_READONLY)
        s->st_mode &= ~0222;
    s->st_size = st->cbFile;
    s->st_mtime = file_time(st->fdateLastWrite, st->ftimeLastWrite);
    s->st_atime = st->fdateLastAccess.date ? file_time(st->fdateLastAccess, st->ftimeLastAccess) : s->st_mtime;
    s->st_ctime = st->fdateCreation.date ? file_time(st->fdateCreation, st->ftimeCreation) : s->st_mtime;
}

int stat(const char *path, struct stat *s) {
    FILESTATUS3 st;
    int drive = path[0] && path[1] == ':' ? (path[0] | 0x20) - 'a' : _getdrive() - 1;
    APIRET rc = DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st);
    if (rc) {
        if (path[0] && path[1] == ':' && (!path[2] || ((path[2] == '\\' || path[2] == '/') && !path[3]))) {
            memset(s, 0, sizeof *s);
            s->st_mode = S_IFDIR | 0755;
            s->st_dev = drive;
            s->st_nlink = 1;
            s->st_attr = FILE_DIRECTORY;
            return 0;
        }
        return _rs2_oserr(rc);
    }
    fill_stat(s, &st, drive);
    return 0;
}

int lstat(const char *path, struct stat *s) {
    return stat(path, s);
}

int fstat(int fd, struct stat *s) {
    ULONG type, attr;
    APIRET rc = DosQueryHType((HFILE)fd, &type, &attr);
    if (rc)
        return _rs2_oserr(rc);
    if ((type & 0xff) != HANDTYPE_FILE) {
        memset(s, 0, sizeof *s);
        s->st_mode = (type & 0xff) == HANDTYPE_PIPE ? S_IFIFO | 0666 : S_IFCHR | 0666;
        s->st_nlink = 1;
        return 0;
    }
    FILESTATUS3 st;
    rc = DosQueryFileInfo((HFILE)fd, FIL_STANDARD, &st, sizeof st);
    if (rc)
        return _rs2_oserr(rc);
    fill_stat(s, &st, _getdrive() - 1);
    off_t len = filelength(fd);
    if (len >= 0)
        s->st_size = len;
    return 0;
}

int chmod(const char *path, mode_t mode) {
    FILESTATUS3 st;
    APIRET rc = DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st);
    if (rc)
        return _rs2_oserr(rc);
    st.attrFile = mode & S_IWUSR ? st.attrFile & ~FILE_READONLY : st.attrFile | FILE_READONLY;
    st.attrFile &= FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED;
    rc = DosSetPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st, 0);
    return rc ? _rs2_oserr(rc) : 0;
}

mode_t umask(mode_t m) {
    static mode_t cur = 022;
    mode_t old = cur;
    cur = m & 0777;
    return old;
}

/* Directories. */
struct _rs2_dir {
    HDIR h;
    int first, done;
    char pattern[PATH_MAX + 4];
    FILEFINDBUF3 buf;
    struct dirent ent;
};

#define FIND_ALL (FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_DIRECTORY | FILE_ARCHIVED)

static int find_first(DIR *d) {
    ULONG n = 1;
    d->h = HDIR_CREATE;
    APIRET rc = DosFindFirst((PCSZ)d->pattern, &d->h, FIND_ALL, &d->buf, sizeof d->buf, &n, FIL_STANDARD);
    d->first = 1;
    d->done = 0;
    if (rc == ERROR_NO_MORE_FILES || (rc == 0 && n == 0)) {
        d->done = 1;
        d->h = 0;
        return 0;
    }
    return rc;
}

DIR *opendir(const char *path) {
    size_t n = strlen(path);
    if (n + 3 > PATH_MAX) {
        errno = ENAMETOOLONG;
        return 0;
    }
    struct stat st;
    if (stat(path, &st))
        return 0;
    if (!S_ISDIR(st.st_mode)) {
        errno = ENOTDIR;
        return 0;
    }
    DIR *d = calloc(1, sizeof *d);
    if (!d)
        return 0;
    memcpy(d->pattern, path, n);
    if (n && path[n - 1] != '\\' && path[n - 1] != '/' && path[n - 1] != ':')
        d->pattern[n++] = '\\';
    d->pattern[n++] = '*';
    d->pattern[n] = 0;
    APIRET rc = find_first(d);
    if (rc) {
        free(d);
        _rs2_oserr(rc);
        return 0;
    }
    return d;
}

struct dirent *readdir(DIR *d) {
    if (d->done)
        return 0;
    if (!d->first) {
        ULONG n = 1;
        APIRET rc = DosFindNext(d->h, &d->buf, sizeof d->buf, &n);
        if (rc || n == 0) {
            d->done = 1;
            return 0;
        }
    }
    d->first = 0;
    struct dirent *e = &d->ent;
    memcpy(e->d_name, d->buf.achName, d->buf.cchName);
    e->d_name[d->buf.cchName] = 0;
    e->d_attr = d->buf.attrFile;
    e->d_type = d->buf.attrFile & FILE_DIRECTORY ? DT_DIR : DT_REG;
    e->d_size = d->buf.cbFile;
    e->d_ino++;
    return e;
}

void rewinddir(DIR *d) {
    if (d->h)
        DosFindClose(d->h);
    find_first(d);
}

int closedir(DIR *d) {
    if (d->h && !d->done)
        DosFindClose(d->h);
    else if (d->h)
        DosFindClose(d->h);
    free(d);
    return 0;
}
