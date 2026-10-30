/* W16DLL.DLL: a Windows DLL for W16TEST. HELLO16 DLL loads it and ends
 * without freeing it; W16TEST then must not find it (GetModuleHandle),
 * and loading it again runs its LibMain in W16TEST's process. Built a
 * second time as W16D43.DLL (the same module name, W16DLL_VALUE 43): a
 * DLL whose file is replaced is loaded anew. */
#include <windows.h>

#ifndef W16DLL_VALUE
#define W16DLL_VALUE 42
#endif

static int value;
static HINSTANCE instance;

int FAR PASCAL LibMain(HINSTANCE hInst, WORD ds, WORD heap, LPSTR cmd)
{
    value = W16DLL_VALUE;
    instance = hInst;
    return 1;
}

int FAR PASCAL __export W16DllValue(void)
{
    return value;
}

/* The instance handle LibMain got. */
HINSTANCE FAR PASCAL __export W16DllInstance(void)
{
    return instance;
}
