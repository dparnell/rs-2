/* Uses MYLIB.DLL (beside it: LIBPATH has ".") */
#include <stdio.h>

int triple(int n);

int main(void) {
    printf("MYLIB.DLL says %d\n", triple(14));
    return 0;
}
