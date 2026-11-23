/* <stdio.h> streams. Text streams (the default; "b" opens binary) turn
 * CR LF into LF when read, with Ctrl+Z ending the file, and LF into CR LF
 * when written; the buffer holds the bytes as they are in the file, so
 * ftell and fseek positions are file offsets. */
#include "libc.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <process.h>

#define F_READ 1
#define F_WRITE 2
#define F_EOF 4
#define F_ERR 8
#define F_TEXT 16
#define F_LBF 32
#define F_NBF 64
#define F_MYBUF 128
#define F_STATIC 256
#define F_BUFSET 512

enum { M_NONE, M_READ, M_WRITE };

struct _rs2_file {
    int fd;
    int flags;
    unsigned char *buf;
    size_t size;
    unsigned char *rpos, *rend;
    unsigned char *wpos;
    int mode;
    int nub;
    unsigned char ub[8];
    _rs2_lock_t lock;
    struct _rs2_file *next;
    int pid;
    char *tmpname;
    unsigned char one;
};

static struct _rs2_file f_err = {2, F_WRITE | F_TEXT | F_STATIC | F_NBF | F_BUFSET};
static struct _rs2_file f_out = {1, F_WRITE | F_TEXT | F_STATIC, .next = &f_err};
static struct _rs2_file f_in = {0, F_READ | F_TEXT | F_STATIC, .next = &f_out};
FILE *const stdin = &f_in;
FILE *const stdout = &f_out;
FILE *const stderr = &f_err;
static FILE *files = &f_in;
static _rs2_lock_t list_lock;

void flockfile(FILE *f) { _rs2_lock(&f->lock); }
void funlockfile(FILE *f) { _rs2_unlock(&f->lock); }
int ftrylockfile(FILE *f) {
    _rs2_lock(&f->lock);
    return 0;
}

static void setup(FILE *f) {
    if (f->buf)
        return;
    if (!(f->flags & F_BUFSET)) {
        f->flags |= F_BUFSET;
        if ((f->flags & F_WRITE) && isatty(f->fd))
            f->flags |= F_LBF;
    }
    if (!(f->flags & F_NBF)) {
        f->buf = malloc(BUFSIZ);
        if (f->buf) {
            f->size = BUFSIZ;
            f->flags |= F_MYBUF;
        } else {
            f->flags |= F_NBF;
        }
    }
    if (!f->buf) {
        f->buf = &f->one;
        f->size = 1;
    }
    f->rpos = f->rend = f->wpos = f->buf;
}

static int flush_w(FILE *f) {
    if (f->mode != M_WRITE)
        return 0;
    size_t n = f->wpos - f->buf;
    f->wpos = f->buf;
    if (n && _rs2_rawwrite(f->fd, f->buf, n) != (long)n) {
        f->flags |= F_ERR;
        return EOF;
    }
    return 0;
}

/* Leaves reading: gives back what was read ahead. */
static void drop_r(FILE *f) {
    if (f->mode == M_READ) {
        long ahead = (f->rend - f->rpos) + f->nub;
        if (ahead)
            lseek(f->fd, -ahead, SEEK_CUR);
        f->rpos = f->rend = f->buf;
        f->nub = 0;
    }
}

static int to_write(FILE *f) {
    if (!(f->flags & F_WRITE)) {
        f->flags |= F_ERR;
        errno = EBADF;
        return -1;
    }
    setup(f);
    if (f->mode != M_WRITE) {
        drop_r(f);
        f->mode = M_WRITE;
        f->wpos = f->buf;
    }
    return 0;
}

static int fill(FILE *f) {
    if (!(f->flags & F_READ)) {
        f->flags |= F_ERR;
        errno = EBADF;
        return EOF;
    }
    setup(f);
    if (f->mode == M_WRITE && flush_w(f))
        return EOF;
    f->mode = M_READ;
    if (f == stdin && (f_out.flags & F_LBF) && f_out.mode == M_WRITE)
        fflush(stdout);
    long n = _rs2_rawread(f->fd, f->buf, f->size);
    if (n <= 0) {
        f->flags |= n < 0 ? F_ERR : F_EOF;
        f->rpos = f->rend = f->buf;
        return EOF;
    }
    f->rpos = f->buf;
    f->rend = f->buf + n;
    return 0;
}

static int rawget(FILE *f) {
    if (f->rpos < f->rend)
        return *f->rpos++;
    if (fill(f))
        return EOF;
    return *f->rpos++;
}

int getc_unlocked(FILE *f) {
    if (f->nub)
        return f->ub[--f->nub];
    if (f->flags & F_EOF)
        return EOF;
    int c = rawget(f);
    if (c == EOF || !(f->flags & F_TEXT))
        return c;
    if (c == 0x1a) {
        f->rpos--;
        f->flags |= F_EOF;
        return EOF;
    }
    if (c == '\r') {
        int d = rawget(f);
        if (d == '\n')
            return '\n';
        if (d != EOF)
            f->rpos--;
        else
            f->flags &= ~F_EOF;
    }
    return c;
}

int fgetc(FILE *f) {
    flockfile(f);
    int c = getc_unlocked(f);
    funlockfile(f);
    return c;
}

int getc(FILE *f) { return fgetc(f); }
int getchar(void) { return fgetc(stdin); }
int getchar_unlocked(void) { return getc_unlocked(stdin); }

int ungetc(int c, FILE *f) {
    if (c == EOF)
        return EOF;
    flockfile(f);
    if (f->nub == (int)sizeof f->ub) {
        funlockfile(f);
        return EOF;
    }
    if (f->mode == M_WRITE)
        flush_w(f);
    f->mode = M_READ;
    f->ub[f->nub++] = (unsigned char)c;
    f->flags &= ~F_EOF;
    funlockfile(f);
    return (unsigned char)c;
}

static int put_raw(FILE *f, unsigned char c) {
    *f->wpos++ = c;
    if (f->wpos == f->buf + f->size)
        return flush_w(f);
    return 0;
}

int putc_unlocked(int c, FILE *f) {
    if (to_write(f))
        return EOF;
    if ((f->flags & F_TEXT) && c == '\n' && put_raw(f, '\r'))
        return EOF;
    if (put_raw(f, (unsigned char)c))
        return EOF;
    if ((f->flags & F_NBF) || ((f->flags & F_LBF) && c == '\n'))
        if (flush_w(f))
            return EOF;
    return (unsigned char)c;
}

int fputc(int c, FILE *f) {
    flockfile(f);
    int r = putc_unlocked(c, f);
    funlockfile(f);
    return r;
}

int putc(int c, FILE *f) { return fputc(c, f); }
int putchar(int c) { return fputc(c, stdout); }
int putchar_unlocked(int c) { return putc_unlocked(c, stdout); }

/* Writes n bytes (already locked); returns how many went. */
static size_t write_bytes(FILE *f, const unsigned char *s, size_t n) {
    if (to_write(f))
        return 0;
    size_t done = 0;
    int text = f->flags & F_TEXT;
    while (done < n) {
        size_t seg = n - done;
        const unsigned char *nl = text ? memchr(s + done, '\n', seg) : 0;
        if (nl)
            seg = nl - (s + done);
        /* A big binary run with an empty buffer goes straight out. */
        if (seg >= f->size && f->wpos == f->buf) {
            long w = _rs2_rawwrite(f->fd, s + done, seg);
            if (w != (long)seg) {
                f->flags |= F_ERR;
                return done + (w > 0 ? w : 0);
            }
        } else {
            for (size_t i = 0; i < seg;) {
                size_t room = f->buf + f->size - f->wpos, k = seg - i < room ? seg - i : room;
                memcpy(f->wpos, s + done + i, k);
                f->wpos += k;
                i += k;
                if (f->wpos == f->buf + f->size && flush_w(f))
                    return done + i;
            }
        }
        done += seg;
        if (nl) {
            if (put_raw(f, '\r') || put_raw(f, '\n'))
                return done;
            done++;
        }
    }
    if (f->flags & F_NBF || ((f->flags & F_LBF) && memchr(s, '\n', n)))
        if (flush_w(f))
            return done;
    return done;
}

size_t _rs2_fwrite_unlocked(const void *p, size_t n, FILE *f) {
    return write_bytes(f, p, n);
}

size_t fwrite(const void *__restrict p, size_t sz, size_t n, FILE *__restrict f) {
    size_t total;
    if (__builtin_mul_overflow(sz, n, &total) || !total)
        return 0;
    flockfile(f);
    size_t w = write_bytes(f, p, total);
    funlockfile(f);
    return w / sz;
}

size_t fread(void *__restrict p, size_t sz, size_t n, FILE *__restrict f) {
    size_t total;
    if (__builtin_mul_overflow(sz, n, &total) || !total)
        return 0;
    unsigned char *d = p;
    size_t got = 0;
    flockfile(f);
    while (got < total && f->nub)
        d[got++] = f->ub[--f->nub];
    if (!(f->flags & F_TEXT)) {
        while (got < total && !(f->flags & F_EOF)) {
            size_t have = f->rend - f->rpos;
            if (have) {
                size_t k = total - got < have ? total - got : have;
                memcpy(d + got, f->rpos, k);
                f->rpos += k;
                got += k;
                continue;
            }
            setup(f);
            if (total - got >= f->size && f->mode != M_WRITE) {
                f->mode = M_READ;
                long r = _rs2_rawread(f->fd, d + got, total - got);
                if (r <= 0) {
                    f->flags |= r < 0 ? F_ERR : F_EOF;
                    break;
                }
                got += r;
                continue;
            }
            if (fill(f))
                break;
        }
    } else {
        while (got < total) {
            /* Plain bytes up to the next CR or Ctrl+Z go in one copy. */
            size_t have = f->rend - f->rpos;
            if (have > 1 && !(f->flags & F_EOF)) {
                size_t k = total - got < have - 1 ? total - got : have - 1;
                size_t i = 0;
                while (i < k && f->rpos[i] != '\r' && f->rpos[i] != 0x1a)
                    i++;
                if (i) {
                    memcpy(d + got, f->rpos, i);
                    f->rpos += i;
                    got += i;
                    continue;
                }
            }
            int c = getc_unlocked(f);
            if (c == EOF)
                break;
            d[got++] = (unsigned char)c;
        }
    }
    funlockfile(f);
    return got / sz;
}

char *fgets(char *__restrict s, int n, FILE *__restrict f) {
    if (n <= 0)
        return 0;
    flockfile(f);
    int i = 0;
    while (i < n - 1) {
        int c = getc_unlocked(f);
        if (c == EOF)
            break;
        s[i++] = (char)c;
        if (c == '\n')
            break;
    }
    funlockfile(f);
    if (i == 0 && n > 1)
        return 0;
    s[i] = 0;
    return s;
}

ssize_t getdelim(char **__restrict lp, size_t *__restrict np, int delim, FILE *__restrict f) {
    if (!lp || !np) {
        errno = EINVAL;
        return -1;
    }
    flockfile(f);
    size_t i = 0;
    for (;;) {
        int c = getc_unlocked(f);
        if (c == EOF && i == 0) {
            funlockfile(f);
            return -1;
        }
        if (i + 2 > *np || !*lp) {
            size_t nn = *np < 64 ? 128 : *np * 2;
            char *nb = realloc(*lp, nn);
            if (!nb) {
                funlockfile(f);
                return -1;
            }
            *lp = nb;
            *np = nn;
        }
        if (c == EOF)
            break;
        (*lp)[i++] = (char)c;
        if (c == delim)
            break;
    }
    (*lp)[i] = 0;
    funlockfile(f);
    return (ssize_t)i;
}

ssize_t getline(char **__restrict lp, size_t *__restrict np, FILE *__restrict f) {
    return getdelim(lp, np, '\n', f);
}

int fputs(const char *__restrict s, FILE *__restrict f) {
    size_t n = strlen(s);
    flockfile(f);
    size_t w = write_bytes(f, (const unsigned char *)s, n);
    funlockfile(f);
    return w == n ? 0 : EOF;
}

int puts(const char *s) {
    flockfile(stdout);
    size_t n = strlen(s);
    int r = write_bytes(stdout, (const unsigned char *)s, n) == n && putc_unlocked('\n', stdout) != EOF ? 0 : EOF;
    funlockfile(stdout);
    return r;
}

int fflush(FILE *f) {
    if (!f) {
        int r = 0;
        _rs2_lock(&list_lock);
        for (FILE *p = files; p; p = p->next)
            if (p->mode == M_WRITE && fflush(p))
                r = EOF;
        _rs2_unlock(&list_lock);
        return r;
    }
    flockfile(f);
    int r = 0;
    if (f->mode == M_WRITE)
        r = flush_w(f);
    else if (f->mode == M_READ && f != stdin)
        drop_r(f), f->mode = M_NONE;
    funlockfile(f);
    return r;
}

int setvbuf(FILE *__restrict f, char *__restrict buf, int mode, size_t size) {
    if (f->mode != M_NONE || mode < _IOFBF || mode > _IONBF)
        return -1;
    if (f->flags & F_MYBUF)
        free(f->buf);
    f->flags &= ~(F_LBF | F_NBF | F_MYBUF);
    f->flags |= F_BUFSET | (mode == _IOLBF ? F_LBF : mode == _IONBF ? F_NBF : 0);
    f->buf = 0;
    if (mode != _IONBF && buf && size) {
        f->buf = (unsigned char *)buf;
        f->size = size;
        f->rpos = f->rend = f->wpos = f->buf;
    } else if (mode != _IONBF && size) {
        f->buf = malloc(size);
        if (f->buf) {
            f->size = size;
            f->flags |= F_MYBUF;
            f->rpos = f->rend = f->wpos = f->buf;
        }
    }
    return 0;
}

void setbuf(FILE *__restrict f, char *__restrict buf) {
    setvbuf(f, buf, buf ? _IOFBF : _IONBF, BUFSIZ);
}

int fileno(FILE *f) { return f->fd; }
void clearerr(FILE *f) { f->flags &= ~(F_EOF | F_ERR); }
int feof(FILE *f) { return !!(f->flags & F_EOF); }
int ferror(FILE *f) { return !!(f->flags & F_ERR); }

static int parse_mode(const char *m, int *oflags, int *fflags) {
    int o, ff;
    switch (*m) {
    case 'r': o = O_RDONLY; ff = F_READ; break;
    case 'w': o = O_WRONLY | O_CREAT | O_TRUNC; ff = F_WRITE; break;
    case 'a': o = O_WRONLY | O_CREAT | O_APPEND; ff = F_WRITE; break;
    default: errno = EINVAL; return -1;
    }
    ff |= F_TEXT;
    for (m++; *m; m++) {
        if (*m == '+') {
            o = (o & ~O_ACCMODE) | O_RDWR;
            ff |= F_READ | F_WRITE;
        } else if (*m == 'b') {
            ff &= ~F_TEXT;
        } else if (*m == 'x') {
            o |= O_EXCL;
        } else if (*m == 'e' || *m == 'N') {
            o |= O_NOINHERIT;
        }
    }
    *oflags = o | O_BINARY;
    *fflags = ff;
    return 0;
}

static FILE *make(int fd, int fflags) {
    FILE *f = calloc(1, sizeof *f);
    if (!f)
        return 0;
    f->fd = fd;
    f->flags = fflags;
    _rs2_lock(&list_lock);
    f->next = files;
    files = f;
    _rs2_unlock(&list_lock);
    return f;
}

FILE *fopen(const char *__restrict path, const char *__restrict mode) {
    int o, ff;
    if (parse_mode(mode, &o, &ff))
        return 0;
    int fd = open(path, o, 0666);
    if (fd < 0)
        return 0;
    FILE *f = make(fd, ff);
    if (!f)
        close(fd);
    return f;
}

FILE *fdopen(int fd, const char *mode) {
    int o, ff;
    if (parse_mode(mode, &o, &ff))
        return 0;
    return make(fd, ff);
}

static int release(FILE *f) {
    int r = 0;
    if (f->mode == M_WRITE && flush_w(f))
        r = EOF;
    if (f->flags & F_MYBUF)
        free(f->buf);
    f->buf = 0;
    f->flags &= ~F_MYBUF;
    f->mode = M_NONE;
    f->nub = 0;
    if (close(f->fd))
        r = EOF;
    if (f->tmpname) {
        unlink(f->tmpname);
        free(f->tmpname);
        f->tmpname = 0;
    }
    return r;
}

static void unlist(FILE *f) {
    _rs2_lock(&list_lock);
    for (FILE **p = &files; *p; p = &(*p)->next)
        if (*p == f) {
            *p = f->next;
            break;
        }
    _rs2_unlock(&list_lock);
}

int fclose(FILE *f) {
    flockfile(f);
    int r = release(f);
    funlockfile(f);
    if (!(f->flags & F_STATIC)) {
        unlist(f);
        free(f);
    }
    return r;
}

FILE *freopen(const char *__restrict path, const char *__restrict mode, FILE *__restrict f) {
    int o, ff;
    if (parse_mode(mode, &o, &ff))
        return 0;
    flockfile(f);
    if (!path) {
        if (f->mode == M_WRITE)
            flush_w(f);
        f->flags = (f->flags & ~(F_TEXT | F_READ | F_WRITE | F_EOF | F_ERR)) | ff;
        funlockfile(f);
        return f;
    }
    int std = f->fd;
    release(f);
    int fd = open(path, o, 0666);
    if (fd >= 0 && (f->flags & F_STATIC) && fd != std) {
        /* stdin, stdout, stderr keep their handle numbers. */
        dup2(fd, std);
        close(fd);
        fd = std;
    }
    f->flags = (f->flags & (F_STATIC | F_BUFSET | F_LBF | F_NBF)) | ff;
    f->fd = fd;
    funlockfile(f);
    if (fd < 0) {
        if (!(f->flags & F_STATIC)) {
            unlist(f);
            free(f);
        }
        return 0;
    }
    return f;
}

int fseeko(FILE *f, off_t off, int whence) {
    flockfile(f);
    if (f->mode == M_WRITE && flush_w(f)) {
        funlockfile(f);
        return -1;
    }
    if (whence == SEEK_CUR && f->mode == M_READ)
        off -= (f->rend - f->rpos) + f->nub;
    f->rpos = f->rend = f->wpos = f->buf;
    f->nub = 0;
    f->mode = M_NONE;
    off_t r = lseek(f->fd, off, whence);
    if (r >= 0)
        f->flags &= ~F_EOF;
    funlockfile(f);
    return r < 0 ? -1 : 0;
}

int fseek(FILE *f, long off, int whence) {
    return fseeko(f, off, whence);
}

off_t ftello(FILE *f) {
    flockfile(f);
    off_t pos = lseek(f->fd, 0, SEEK_CUR);
    if (pos >= 0) {
        if (f->mode == M_READ)
            pos -= (f->rend - f->rpos) + f->nub;
        else if (f->mode == M_WRITE)
            pos += f->wpos - f->buf;
    }
    funlockfile(f);
    return pos;
}

long ftell(FILE *f) {
    return ftello(f);
}

void rewind(FILE *f) {
    fseeko(f, 0, SEEK_SET);
    clearerr(f);
}

int fgetpos(FILE *__restrict f, fpos_t *__restrict pos) {
    off_t p = ftello(f);
    if (p < 0)
        return -1;
    *pos = p;
    return 0;
}

int fsetpos(FILE *f, const fpos_t *pos) {
    return fseeko(f, *pos, SEEK_SET);
}

void _rs2_stdio_exit(void) {
    /* Flush everything; close temporary files so they go away. */
    for (FILE *p = files; p; p = p->next) {
        if (p->mode == M_WRITE)
            flush_w(p);
        if (p->tmpname)
            release(p);
    }
}

/* Temporary files: in TMP (or TEMP, or the current directory). */
static int tmp_counter;

static void tmp_name(char *buf, size_t n, const char *pattern_dir) {
    const char *dir = pattern_dir ? pattern_dir : getenv("TMP");
    if (!dir)
        dir = getenv("TEMP");
    int c = __atomic_add_fetch(&tmp_counter, 1, __ATOMIC_RELAXED);
    snprintf(buf, n, "%s%s%04x%04x.TMP", dir ? dir : "", dir && *dir && dir[strlen(dir) - 1] != '\\' ? "\\" : "",
             getpid() & 0xffff, c & 0xffff);
}

char *tmpnam(char *buf) {
    char *b = buf ? buf : _rs2_self()->tmpnam;
    for (int i = 0; i < 1000; i++) {
        tmp_name(b, L_tmpnam, 0);
        if (access(b, F_OK) != 0)
            return b;
    }
    return 0;
}

FILE *tmpfile(void) {
    char name[300];
    for (int i = 0; i < 1000; i++) {
        tmp_name(name, sizeof name, 0);
        int fd = open(name, O_RDWR | O_CREAT | O_EXCL | O_BINARY | O_NOINHERIT, 0600);
        if (fd < 0) {
            if (errno == EEXIST)
                continue;
            return 0;
        }
        FILE *f = make(fd, F_READ | F_WRITE);
        if (!f) {
            close(fd);
            unlink(name);
            return 0;
        }
        f->tmpname = strdup(name);
        return f;
    }
    return 0;
}

char *mktemp(char *t) {
    size_t n = strlen(t);
    if (n < 6 || strcmp(t + n - 6, "XXXXXX")) {
        errno = EINVAL;
        t[0] = 0;
        return t;
    }
    for (int i = 0; i < 10000; i++) {
        int c = __atomic_add_fetch(&tmp_counter, 1, __ATOMIC_RELAXED);
        snprintf(t + n - 6, 7, "%02x%04x", getpid() & 0xff, c & 0xffff);
        if (access(t, F_OK) != 0)
            return t;
    }
    t[0] = 0;
    return t;
}

int mkstemp(char *t) {
    size_t n = strlen(t);
    for (int i = 0; i < 1000; i++) {
        if (n >= 6)
            memcpy(t + n - 6, "XXXXXX", 6);
        if (!*mktemp(t))
            return -1;
        int fd = open(t, O_RDWR | O_CREAT | O_EXCL | O_BINARY, 0600);
        if (fd >= 0 || errno != EEXIST)
            return fd;
    }
    return -1;
}

/* popen: the command runs under the shell with one end of a pipe as its
 * standard input or output. */
FILE *popen(const char *cmd, const char *mode) {
    int reading = *mode == 'r';
    if (!reading && *mode != 'w') {
        errno = EINVAL;
        return 0;
    }
    int fds[2];
    if (pipe(fds))
        return 0;
    int child = reading ? fds[1] : fds[0], mine = reading ? fds[0] : fds[1], target = reading ? 1 : 0;
    fflush(0);
    int saved = dup(target);
    fcntl(mine, F_SETFD, FD_CLOEXEC);
    if (saved >= 0)
        fcntl(saved, F_SETFD, FD_CLOEXEC);
    dup2(child, target);
    const char *shell = getenv("COMSPEC");
    if (!shell)
        shell = "CMD.EXE";
    char *argv[] = {(char *)shell, "/c", (char *)cmd, 0};
    /* The command goes to the shell as one string, unquoted. */
    size_t n = strlen(shell), c = strlen(cmd);
    char *args = _rs2_low_alloc(n + c + 8), *p = args;
    char *env = _rs2_env_block();
    int pid = -1;
    (void)argv;
    if (args && env) {
        p = stpcpy(p, shell) + 1;
        p = stpcpy(p, "/c ");
        p = stpcpy(p, cmd) + 1;
        *p = 0;
        char failed[260];
        RESULTCODES rc;
        APIRET r = DosExecPgm((PCHAR)failed, sizeof failed, EXEC_ASYNCRESULT, (PCSZ)args, (PCSZ)env, &rc, LOW(shell));
        if (r)
            _rs2_oserr(r);
        else
            pid = (int)rc.codeTerminate;
    }
    free(args);
    free(env);
    if (saved >= 0) {
        dup2(saved, target);
        close(saved);
    }
    close(child);
    if (pid < 0) {
        close(mine);
        return 0;
    }
    FILE *f = make(mine, (reading ? F_READ : F_WRITE) | F_TEXT);
    if (!f) {
        close(mine);
        return 0;
    }
    f->pid = pid;
    return f;
}

int pclose(FILE *f) {
    int pid = f->pid;
    fclose(f);
    if (pid <= 0)
        return -1;
    int st;
    if (cwait(&st, pid, 0) < 0)
        return -1;
    return st;
}
