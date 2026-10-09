typedef void (*Callback)(int *);
extern void decrement(int *);
void unknown_write(Callback *slot) { *slot = decrement; }
