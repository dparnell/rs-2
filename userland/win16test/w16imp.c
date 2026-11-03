/* W16IMP.DLL: a Windows DLL W16TEST imports (not LoadLibrary): the
 * loader loads it with the program and runs its LibMain first. */
#include <windows.h>

static int started;
static HINSTANCE instance;

int FAR PASCAL LibMain(HINSTANCE hInst, WORD ds, WORD heap, LPSTR cmd)
{
    started = 7;
    instance = hInst;
    return 1;
}

int FAR PASCAL __export W16ImpValue(void)
{
    return started;
}

HINSTANCE FAR PASCAL __export W16ImpInstance(void)
{
    return instance;
}
