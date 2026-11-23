/* clang++: CLANG.EXE (next to this program) with --driver-mode=g++. */
#include <process.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    char path[300];
    strlcpy(path, getexecname(), sizeof path - 16);
    char *slash = strrchr(path, '\\');
    strcpy(slash ? slash + 1 : path, "CLANG.EXE");
    char **args = malloc((size_t)(argc + 2) * sizeof *args);
    if (!args)
        return 1;
    args[0] = path;
    args[1] = "--driver-mode=g++";
    memcpy(args + 2, argv + 1, (size_t)argc * sizeof *args);   /* and the null */
    int r = spawnv(P_WAIT, path, args);
    if (r < 0)
        perror(path);
    return r < 0 ? 1 : r;
}
