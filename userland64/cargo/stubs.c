/* RSCARGO.DLL's curl and libgit2 for cargo (docs/SELF-HOSTING.md): there
 * is no network and no git yet, so most calls fail. Those cargo makes in
 * any case succeed and do nothing. build.sh adds a failing function (-1)
 * for each further one that cargo imports. */
#include <stddef.h>
#include <string.h>

#define API __attribute__((visibility("default")))

/* libgit2: GIT_ENOTFOUND where cargo looks for a repository. */
struct git_error {
    char *message;
    int klass;
};
static struct git_error none = {"git is not available in this cargo", 0};
API const struct git_error *git_error_last(void) { return &none; }
API void git_error_clear(void) {}
API int git_libgit2_init(void) { return 1; }
API int git_libgit2_opts(int o, ...) { return 0; }
API int git_libgit2_features(void) { return 0; }
API int git_libgit2_version(int *major, int *minor, int *rev) {
    *major = 1, *minor = 9, *rev = 6;
    return 0;
}
API int git_transport_register(const char *prefix, void *cb, void *param) { return 0; }
API int git_repository_discover(void *out, const char *start, int across, const char *ceiling) { return -3; }
API int git_repository_open(void **out, const char *path) { return -3; }
API int git_repository_open_ext(void **out, const char *path, unsigned flags, const char *ceiling) { return -3; }
API int git_config_open_default(void **out) { return -3; }

/* curl: handles that cannot be used. */
static char easy, multi;
API int curl_global_init(long flags) { return 0; }
API void *curl_easy_init(void) { return &easy; }
API int curl_easy_setopt(void *h, int o, ...) { return 0; }
API int curl_easy_getinfo(void *h, int o, ...) { return 48; }
API void curl_easy_reset(void *h) {}
API void curl_easy_cleanup(void *h) {}
API int curl_easy_perform(void *h) { return 1; }
API const char *curl_easy_strerror(int e) { return "the network is not available in this cargo"; }
API void *curl_multi_init(void) { return &multi; }
API int curl_multi_setopt(void *h, int o, ...) { return 0; }
API int curl_multi_cleanup(void *h) { return 0; }
API int curl_multi_add_handle(void *m, void *h) { return 1; }
API int curl_multi_remove_handle(void *m, void *h) { return 0; }
API const char *curl_multi_strerror(int e) { return "the network is not available in this cargo"; }
API void *curl_multi_info_read(void *m, int *left) {
    *left = 0;
    return 0;
}
API void *curl_slist_append(void *l, const char *s) { return l ? l : &easy; }
API void curl_slist_free_all(void *l) {}
API void curl_formfree(void *f) {}

struct version_info {
    int age;
    const char *version;
    unsigned version_num;
    const char *host;
    int features;
    const char *ssl_version;
    long ssl_version_num;
    const char *libz_version;
    const char *const *protocols;
    void *more[16];
};
API const struct version_info *curl_version_info(int age) {
    static const char *const protocols[] = {0};
    static const struct version_info v = {0, "none", 0, "rs2", 0, 0, 0, 0, protocols};
    return &v;
}

/* SQLite locks with lock files: the byte range locks of its default way,
 * changed from shared to exclusive, are not OS/2's. */
extern int sqlite3_initialize(void);
extern int sqlite3_vfs_register(void *, int);
extern void *sqlite3_vfs_find(const char *);
__attribute__((constructor)) static void locking(void) {
    if (sqlite3_initialize() == 0)
        sqlite3_vfs_register(sqlite3_vfs_find("unix-dotfile"), 1);
}
