/* W16CYC1.DLL and W16CYC2.DLL import each other, as Resource Workshop's
 * WORKLIB1 and WORKLIB2 do. LibMain registers a window message: a DLL's
 * LibMain may use USER before its program's InitApp (BWCC does). */
#include <windows.h>

int FAR PASCAL W16Cyc2Value(void);

static UINT msg;

int FAR PASCAL LibMain(HINSTANCE hInst, WORD ds, WORD heap, LPSTR cmd)
{
    msg = RegisterWindowMessage("W16CYC1");
    return 1;
}

int FAR PASCAL __export W16Cyc1Base(void)
{
    return 5;
}

/* W16CYC2's value (W16Cyc1Base() * 2) and one. */
int FAR PASCAL __export W16Cyc1Value(void)
{
    return W16Cyc2Value() + 1;
}

UINT FAR PASCAL __export W16Cyc1Message(void)
{
    return msg;
}
