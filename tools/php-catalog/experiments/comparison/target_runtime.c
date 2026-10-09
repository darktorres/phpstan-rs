#include <stdint.h>
#include <unistd.h>
/* Keep this uninstrumented. A single write records a positive target witness. */
void __php_catalog_target(uint64_t site, void *target) {
    static const char digits[] = "0123456789abcdef";
    char buffer[39] = "PTA ";
    uintptr_t address = (uintptr_t)target;
    for (unsigned i = 0; i < 16; ++i) {
        buffer[4 + i] = digits[(site >> ((15 - i) * 4)) & 15];
        buffer[21 + i] = digits[(address >> ((15 - i) * 4)) & 15];
    }
    buffer[20] = ' '; buffer[37] = '\n';
    (void)write(STDERR_FILENO, buffer, 38);
}
