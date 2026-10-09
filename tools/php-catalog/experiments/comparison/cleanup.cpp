extern void event(int);
extern void may_throw();
struct Guard { ~Guard() { event(2); } };
extern "C" void cleanup() {
  try {
    Guard guard;
    event(1);
    may_throw();
    event(3);
  } catch (...) {
    event(4);
  }
}
