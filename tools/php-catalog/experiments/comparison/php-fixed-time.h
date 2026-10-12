/* Fixes the build date and time so the php binary does not record when it was built. */
#undef __DATE__
#undef __TIME__
#define __DATE__ "Jan 01 2000"
#define __TIME__ "00:00:00"
