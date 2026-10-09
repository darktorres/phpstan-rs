/* Independent reduced contracts: these are not source-derived PHP summaries. */
#include <assert.h>
#include <stddef.h>
#include <string.h>
#ifdef __CPROVER__
extern unsigned nondet_uint(void);
#define ASSUME(x) __CPROVER_assume(x)
#else
static unsigned nondet_uint(void) { return 2; }
#define ASSUME(x) assert(x)
#endif
typedef int (*Callback)(int *);
static int events[16], count, pending;
static Callback retained;
static int *retained_argument;
static int increment(int *x) { events[count++] = 1; return ++*x; }
static int throwing(int *x) { events[count++] = 1; ++*x; pending = 1; return 0; }
static void release_value(void) { events[count++] = 2; }
static int invoke(Callback callback, int *x) {
    int result = callback(x);
    release_value();
    if (pending) return -1;
    events[count++] = 3;
    return result;
}
static void alias_write(int *left, int *right) { *left = 1; *right = 2; }
typedef struct { unsigned char bytes[64]; Callback callback; } Payload;
static int through_copy(Payload *source, int *value) {
    Payload destination;
    memcpy(&destination, source, sizeof(destination));
    return destination.callback(value);
}
int main(void) {
    unsigned length = nondet_uint(); ASSUME(length <= 3);
    int value = 0, results[3] = {0};
    for (unsigned i = 0; i < length; ++i) results[i] = increment(&value);
    assert(value == (int)length);
    assert(count == (int)length);
    for (unsigned i = 0; i < length; ++i) assert(results[i] == (int)i + 1);
    count = 0; value = 0;
    assert(invoke(throwing, &value) == -1);
    assert(value == 1 && count == 2 && events[0] == 1 && events[1] == 2);
    pending = 0; count = 0; value = 0;
    assert(invoke(increment, &value) == 1);
    assert(count == 3 && events[0] == 1 && events[1] == 2 && events[2] == 3);
    alias_write(&value, &value); assert(value == 2);
    Payload source = {{0}, increment};
    assert(through_copy(&source, &value) == 3);
    retained = increment; retained_argument = &value;
    assert(retained(retained_argument) == 4);
#ifdef INVALID_COMPOSITION
    /* A trusted return bound cannot erase callback writes or throws. */
    count = 0; value = 0; pending = 0;
    invoke(throwing, &value);
    assert(value == 0 && pending == 0);
#endif
    return 0;
}
