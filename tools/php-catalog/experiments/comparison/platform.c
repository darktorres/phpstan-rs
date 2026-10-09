// Older PHP/GNU and target-sensitive native constructs, without host headers.
static __thread int thread_state;
int statement_expression(int value) {
  return ({ int copy = value; ++copy; });
}
void computed_goto(unsigned which) {
  static void *labels[] = {&&first, &&second};
  if (which > 1) return;
  goto *labels[which];
first: thread_state = 1; return;
second: thread_state = 2;
}
int atomic_read(_Atomic(int) *value) { return __atomic_load_n((int *)value, __ATOMIC_SEQ_CST); }
extern int engine_setjmp(void **) __attribute__((returns_twice));
extern void engine_bailout(void **) __attribute__((noreturn));
int bailout_entry(void **state, int fail) {
  if (engine_setjmp(state)) return -1;
  if (fail) engine_bailout(state);
  return 0;
}
