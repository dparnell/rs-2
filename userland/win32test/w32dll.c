/* W32DLL.DLL: a Win32 DLL for W32TEST. Counts its DllMain calls; exports
   a function, a variable and an ordinal-only entry. */
#include <windows.h>

__declspec(dllexport) LONG W32DllAttaches = 0;
__declspec(dllexport) LONG W32DllThreadAttaches = 0;
__declspec(dllexport) LONG W32DllThreadDetaches = 0;
static int initialised = 0;

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved)
{
    (void)h; (void)reserved;
    switch (reason) {
    case DLL_PROCESS_ATTACH: W32DllAttaches++; initialised = 1; break;
    case DLL_THREAD_ATTACH: W32DllThreadAttaches++; break;
    case DLL_THREAD_DETACH: W32DllThreadDetaches++; break;
    }
    return TRUE;
}

__declspec(dllexport) int WINAPI W32DllAdd(int a, int b)
{
    return initialised ? a + b : -1;
}

__declspec(dllexport) HMODULE WINAPI W32DllSelf(void)
{
    MEMORY_BASIC_INFORMATION mbi;
    VirtualQuery((void *)W32DllSelf, &mbi, sizeof mbi);
    return (HMODULE)mbi.AllocationBase;
}
