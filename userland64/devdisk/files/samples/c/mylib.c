/* A DLL: what is visible is exported. */
__attribute__((visibility("default"))) int triple(int n) { return 3 * n; }
