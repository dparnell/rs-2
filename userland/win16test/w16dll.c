/* W16DLL.DLL: a Windows DLL for W16TEST. HELLO16 DLL loads it and ends
 * without freeing it; W16TEST then must not find it (GetModuleHandle),
 * and loading it again runs its LibMain in W16TEST's process. */
#include <windows.h>

static int value;

int FAR PASCAL LibMain(HINSTANCE hInst, WORD ds, WORD heap, LPSTR cmd)
{
    value = 42;
    return 1;
}

int FAR PASCAL __export W16DllValue(void)
{
    return value;
}
