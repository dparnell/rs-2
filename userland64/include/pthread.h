#ifndef _PTHREAD_H
#define _PTHREAD_H
#include <sys/types.h>
#include <time.h>
#include <sched.h>
_RS2_BEGIN
/* POSIX threads on OS/2 threads (docs/LIBC64.md). */
typedef struct _rs2_pthread *pthread_t;
typedef struct { int state, type, count, pad; void *owner; } pthread_mutex_t;
typedef struct { int type; } pthread_mutexattr_t;
typedef struct { int seq, clock; } pthread_cond_t;
typedef struct { int clock; } pthread_condattr_t;
typedef struct { int state; int waiters; } pthread_rwlock_t;
typedef struct { int unused; } pthread_rwlockattr_t;
typedef struct { int state; } pthread_once_t;
typedef struct { size_t stack; int detached; } pthread_attr_t;
typedef unsigned pthread_key_t;
#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_DEFAULT 0
#define PTHREAD_MUTEX_RECURSIVE 1
#define PTHREAD_MUTEX_ERRORCHECK 2
#define PTHREAD_MUTEX_INITIALIZER {0, 0, 0, 0, 0}
#define PTHREAD_RECURSIVE_MUTEX_INITIALIZER {0, PTHREAD_MUTEX_RECURSIVE, 0, 0, 0}
#define PTHREAD_COND_INITIALIZER {0, 0}
#define PTHREAD_RWLOCK_INITIALIZER {0, 0}
#define PTHREAD_ONCE_INIT {0}
#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1
#define PTHREAD_KEYS_MAX 128
#define PTHREAD_STACK_MIN 65536
int pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
_RS2_NORETURN void pthread_exit(void *);
int pthread_join(pthread_t, void **);
int pthread_detach(pthread_t);
pthread_t pthread_self(void);
int pthread_equal(pthread_t, pthread_t);
int pthread_attr_init(pthread_attr_t *);
int pthread_attr_destroy(pthread_attr_t *);
int pthread_attr_setstacksize(pthread_attr_t *, size_t);
int pthread_attr_getstacksize(const pthread_attr_t *, size_t *);
int pthread_attr_setdetachstate(pthread_attr_t *, int);
int pthread_mutexattr_init(pthread_mutexattr_t *);
int pthread_mutexattr_destroy(pthread_mutexattr_t *);
int pthread_mutexattr_settype(pthread_mutexattr_t *, int);
int pthread_mutex_init(pthread_mutex_t *, const pthread_mutexattr_t *);
int pthread_mutex_destroy(pthread_mutex_t *);
int pthread_mutex_lock(pthread_mutex_t *);
int pthread_mutex_trylock(pthread_mutex_t *);
int pthread_mutex_unlock(pthread_mutex_t *);
int pthread_condattr_init(pthread_condattr_t *);
int pthread_condattr_destroy(pthread_condattr_t *);
int pthread_condattr_setclock(pthread_condattr_t *, clockid_t);
int pthread_cond_init(pthread_cond_t *, const pthread_condattr_t *);
int pthread_cond_destroy(pthread_cond_t *);
int pthread_cond_wait(pthread_cond_t *, pthread_mutex_t *);
int pthread_cond_timedwait(pthread_cond_t *, pthread_mutex_t *, const struct timespec *);
int pthread_cond_signal(pthread_cond_t *);
int pthread_cond_broadcast(pthread_cond_t *);
int pthread_rwlock_init(pthread_rwlock_t *, const pthread_rwlockattr_t *);
int pthread_rwlock_destroy(pthread_rwlock_t *);
int pthread_rwlock_rdlock(pthread_rwlock_t *);
int pthread_rwlock_tryrdlock(pthread_rwlock_t *);
int pthread_rwlock_wrlock(pthread_rwlock_t *);
int pthread_rwlock_trywrlock(pthread_rwlock_t *);
int pthread_rwlock_unlock(pthread_rwlock_t *);
int pthread_once(pthread_once_t *, void (*)(void));
int pthread_key_create(pthread_key_t *, void (*)(void *));
int pthread_key_delete(pthread_key_t);
void *pthread_getspecific(pthread_key_t);
int pthread_setspecific(pthread_key_t, const void *);
/* What the above wait with: sleeps while *addr (4 or 8 bytes) is
 * `expected`, at most ms milliseconds (-1: for ever). 0, EAGAIN (it was
 * not) or ETIMEDOUT; may wake without reason. __rs2_wake wakes up to n. */
int __rs2_wait(const volatile void *addr, unsigned long long expected, int size, long ms);
int __rs2_wake(const volatile void *addr, int n);
_RS2_END
#endif
