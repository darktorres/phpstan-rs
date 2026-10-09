#include <assert.h>
#include <stddef.h>
#include <string.h>
typedef int (*Callback)(int);
int first(int x) { return x + 1; }
int second(int x) { return x + 2; }
typedef struct { char prefix[7][9]; Callback callbacks[3]; int value; } Nested;
typedef struct { Nested nested[2]; Callback tail; } Outer;
static Outer global = {{{{{0}}, {first, second, first}, 0}, {{{0}}, {second, first, second}, 0}}, second};
int nested_copy(void) {
    Outer copied;
    memcpy(&copied, &global, sizeof(copied));
    return copied.nested[1].callbacks[2](0) + copied.tail(0);
}
int overlap_copy(void) {
    Callback slots[3] = {first, second, first};
    memmove(&slots[1], &slots[0], 2 * sizeof(Callback));
    return slots[2](0);
}
int negative_index(void) {
    Callback *end = &global.nested[1].callbacks[2];
    return end[-1](0);
}
int main(void) {
    assert(nested_copy() == 4);
    assert(overlap_copy() == 2);
    assert(negative_index() == 1);
    return 0;
}
