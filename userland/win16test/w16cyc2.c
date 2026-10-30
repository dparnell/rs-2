/* W16CYC2.DLL: see W16CYC1.C. */
#include <windows.h>

int FAR PASCAL W16Cyc1Base(void);

int FAR PASCAL LibMain(HINSTANCE hInst, WORD ds, WORD heap, LPSTR cmd)
{
    return 1;
}

int FAR PASCAL __export W16Cyc2Value(void)
{
    return W16Cyc1Base() * 2;
}
