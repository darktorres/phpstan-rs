#include <assert.h>
extern unsigned nondet_uint(void);
int main(void) {
    unsigned length = nondet_uint();
    __CPROVER_assume(length < 100);
    unsigned count = 0;
    for (unsigned i = 0; i < length; ++i) ++count;
    assert(count == length);
    return 0;
}
