#ifndef _SPAWN_H
#define _SPAWN_H
#include <sys/types.h>
_RS2_BEGIN
typedef struct {
    int count;
    void *actions;
} posix_spawn_file_actions_t;
typedef struct {
    short flags;
} posix_spawnattr_t;
int posix_spawn(pid_t *__restrict, const char *__restrict, const posix_spawn_file_actions_t *, const posix_spawnattr_t *__restrict,
                char *const *__restrict, char *const *__restrict);
int posix_spawnp(pid_t *__restrict, const char *__restrict, const posix_spawn_file_actions_t *, const posix_spawnattr_t *__restrict,
                 char *const *__restrict, char *const *__restrict);
int posix_spawn_file_actions_init(posix_spawn_file_actions_t *);
int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *);
int posix_spawn_file_actions_addopen(posix_spawn_file_actions_t *__restrict, int, const char *__restrict, int, mode_t);
int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *, int, int);
int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *, int);
int posix_spawnattr_init(posix_spawnattr_t *);
int posix_spawnattr_destroy(posix_spawnattr_t *);
int posix_spawnattr_setflags(posix_spawnattr_t *, short);
_RS2_END
#endif
