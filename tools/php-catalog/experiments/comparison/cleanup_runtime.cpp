#include <cstdio>
extern "C" void cleanup();
static bool throwing;
void event(int value) { std::printf("%d", value); }
void may_throw() { if (throwing) throw 1; }
int main() {
  throwing = false; cleanup(); std::puts("");
  throwing = true; cleanup(); std::puts("");
}
