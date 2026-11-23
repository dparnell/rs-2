#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(int argc, char **argv) {
    char *p = malloc(100);
    strcpy(p, "linked on rs2");
    printf("hello: %s, %d argument(s)\n", p, argc - 1);
    return 42;
}
