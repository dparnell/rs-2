#include <stdio.h>
int main(int c, char **v) {
    FILE *a = fopen(v[1], "rb"), *b = fopen(v[2], "rb");
    long n = 0;
    if (!a || !b) { printf("cmp: cannot open\n"); return 2; }
    for (;;) {
        int x = getc(a), y = getc(b);
        if (x != y) { printf("%s and %s differ at %ld\n", v[1], v[2], n); return 1; }
        if (x == EOF) { printf("%s and %s are the same, %ld bytes\n", v[1], v[2], n); return 0; }
        n++;
    }
}
