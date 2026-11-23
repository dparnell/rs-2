/* The environment: getenv, setenv, putenv, unsetenv. */
#include "libc.h"
#include <stdlib.h>
#include <string.h>

char **environ;
char **_rs2_argv;
int _rs2_environ_owned;
#define environ_owned _rs2_environ_owned

static size_t name_len(const char *s) {
    const char *e = strchr(s, '=');
    return e ? (size_t)(e - s) : strlen(s);
}

static char **find(const char *name, size_t n) {
    if (!environ)
        return 0;
    for (char **e = environ; *e; e++)
        if (!strncmp(*e, name, n) && (*e)[n] == '=')
            return e;
    return 0;
}

char *getenv(const char *name) {
    if (!environ) {
        /* In a DLL, or before start-up: ask the system. */
        PSZ v = 0;
        return DosScanEnv(LOW(name), (PPVOID)&v) == 0 ? (char *)v : 0;
    }
    char **e = find(name, strlen(name));
    return e ? *e + strlen(name) + 1 : 0;
}

static _rs2_lock_t env_lock;

static int set(char *entry) {
    size_t n = name_len(entry);
    char **e = find(entry, n);
    if (e) {
        *e = entry;
        return 0;
    }
    size_t count = 0;
    if (environ)
        while (environ[count])
            count++;
    char **ne = malloc(sizeof(char *) * (count + 2));
    if (!ne)
        return -1;
    if (environ)
        memcpy(ne, environ, sizeof(char *) * count);
    ne[count] = entry;
    ne[count + 1] = 0;
    if (environ_owned)
        free(environ);
    environ = ne;
    environ_owned = 1;
    return 0;
}

int putenv(char *s) {
    if (!strchr(s, '='))
        return unsetenv(s);
    _rs2_lock(&env_lock);
    int r = set(s);
    _rs2_unlock(&env_lock);
    return r;
}

int setenv(const char *name, const char *value, int replace) {
    if (!name || !*name || strchr(name, '=')) {
        errno = EINVAL;
        return -1;
    }
    _rs2_lock(&env_lock);
    if (!replace && find(name, strlen(name))) {
        _rs2_unlock(&env_lock);
        return 0;
    }
    size_t n = strlen(name), v = strlen(value);
    char *s = malloc(n + v + 2);
    if (!s) {
        _rs2_unlock(&env_lock);
        return -1;
    }
    memcpy(s, name, n);
    s[n] = '=';
    memcpy(s + n + 1, value, v + 1);
    int r = set(s);
    _rs2_unlock(&env_lock);
    return r;
}

int unsetenv(const char *name) {
    _rs2_lock(&env_lock);
    char **e = find(name, strlen(name));
    if (e)
        for (; *e; e++)
            e[0] = e[1];
    _rs2_unlock(&env_lock);
    return 0;
}

/* An environment block for DosExecPgm, from environ. */
char *_rs2_env_block(void) {
    size_t len = 1;
    if (environ)
        for (char **e = environ; *e; e++)
            len += strlen(*e) + 1;
    char *b = _rs2_low_alloc(len + 1), *p = b;
    if (!b)
        return 0;
    if (environ)
        for (char **e = environ; *e; e++)
            p = stpcpy(p, *e) + 1;
    *p++ = 0;
    *p = 0;
    return b;
}

