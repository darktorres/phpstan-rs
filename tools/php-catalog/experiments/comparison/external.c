// A complete native witness is linked with external_impl.c. An analyzer seeing
// this module alone must keep the post-call target open or model the write.
typedef void (*Callback)(int *);
extern void unknown_write(Callback *);
void increment(int *value) { ++*value; }
void decrement(int *value) { --*value; }
void external_write_entry(int *value) {
  Callback callback = increment;
  unknown_write(&callback);
  callback(value);
}
int main(void) {
  int value = 100;
  external_write_entry(&value);
  return value != 99;
}
