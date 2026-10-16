PHP_ARG_ENABLE([cattrace], [whether to enable cattrace], [AS_HELP_STRING([--enable-cattrace], [Enable cattrace])], [no])
if test "$PHP_CATTRACE" != "no"; then
  PHP_NEW_EXTENSION(cattrace, cattrace.c, $ext_shared)
fi
