// Reduced Zend-like operations. These are sentinels, not models of php-src.
#include <stddef.h>
#include <string.h>
typedef void (*Callback)(int *);
typedef struct Owner { Callback callback; int *argument; } Owner;
#ifndef PAYLOAD_BYTES
#define PAYLOAD_BYTES 65536
#endif
typedef struct Payload { char bytes[PAYLOAD_BYTES]; Callback callback; } Payload;
extern void release_value(int *);
extern void observe(int);
extern void *unread_library(void *);
int pending_exception;
Owner retained;
void increment(int *value) { ++*value; }
void decrement(int *value) { --*value; }
int scalar(int x) { return x + 1; }
void array_callback(Callback cb, int *values, size_t count) {
  if (!cb || !count) return;
  for (size_t i = 0; i < count; ++i) cb(values + i);
}
void by_reference(int *left, int *right) { *left = 1; *right = 2; }
int exception_cleanup(Callback cb, int *argument) {
  cb(argument);
  release_value(argument);
  if (pending_exception) return -1;
  observe(*argument);
  return 0;
}
void owner_dispatch(Owner *owner) { owner->callback(owner->argument); }
void retain(Callback cb, int *argument) {
  retained.callback = cb; retained.argument = argument;
}
void trigger(void) { retained.callback(retained.argument); }
void copy_payload(Payload *dst, Payload *src) { memcpy(dst, src, sizeof(*src)); }
void copied_dispatch(Payload *src, int *argument) {
  Payload dst; copy_payload(&dst, src); dst.callback(argument);
}
void unknown_copy(Payload *dst, Payload *src) {
  void *unknown = unread_library(src); memcpy(dst, unknown, sizeof(*dst));
}
void ordered_dispatch(int *argument) {
  Owner owner = {increment, argument};
  owner_dispatch(&owner);
  owner.callback = decrement;
  owner_dispatch(&owner);
}
int main(void) {
  int value = 0;
  Callback cb = increment;
  array_callback(cb, &value, 1);
  by_reference(&value, &value);
  exception_cleanup(cb, &value);
  Owner owner = {increment, &value}; owner_dispatch(&owner);
  retain(increment, &value); trigger();
  Payload src = {{0}, increment}; copied_dispatch(&src, &value);
  Payload dst; unknown_copy(&dst, &src);
  ordered_dispatch(&value);
  return scalar(value);
}
