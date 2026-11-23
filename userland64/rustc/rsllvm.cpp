// RSLLVM.DLL: what rustc's LLVM wrapper needs from rustc itself.
#include <stddef.h>

typedef void (*WriteFn)(void *buf, const char *p, size_t n);
static WriteFn string_write;

extern "C" __attribute__((visibility("default"))) void LLVMRustRS2SetStringWrite(WriteFn f) { string_write = f; }
extern "C" void LLVMRustStringWriteImpl(void *buf, const char *p, size_t n) { string_write(buf, p, n); }
