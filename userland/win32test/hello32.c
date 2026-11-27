/* HELLO32: the Win32 hello world (docs/WIN32.md). Prints its arguments
   and PATH and ends with exit code 3. */
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
    int i;
    printf("hello32: hello from a Win32 program, %d argument(s)\n", argc);
    for (i = 0; i < argc; i++)
        printf("  argv[%d] = [%s]\n", i, argv[i]);
    printf("  PATH = %s\n", getenv("PATH") ? getenv("PATH") : "(none)");
    return 3;
}
