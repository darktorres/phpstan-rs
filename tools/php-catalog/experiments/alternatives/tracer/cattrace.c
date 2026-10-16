/* Records, for each internal function call, the user code that runs directly inside it:
 * the user function, its class and method, and how its object or argument relates to the
 * internal call's arguments. Aggregated per process and appended to cattrace.output at MSHUTDOWN. */
#ifdef HAVE_CONFIG_H
# include "config.h"
#endif
#include "php.h"
#include "php_ini.h"
#include "SAPI.h"
#include "zend_observer.h"
#include "zend_closures.h"
#include "zend_smart_str.h"
#include <fcntl.h>
#include <unistd.h>

#define MAX_DEPTH 8192

typedef struct { zend_execute_data *ex; bool internal; } frame;
static frame stack[MAX_DEPTH];
static int depth = 0;
static HashTable counts;
static bool counts_ready = false;
static char script[1024] = "";

PHP_INI_BEGIN()
	PHP_INI_ENTRY("cattrace.output", "", PHP_INI_ALL, NULL)
PHP_INI_END()

static void append_name(smart_str *s, zend_function *f)
{
	if (f->common.scope) {
		smart_str_append(s, f->common.scope->name);
		smart_str_appends(s, "::");
	}
	if (f->common.function_name) {
		smart_str_append(s, f->common.function_name);
	} else {
		smart_str_appends(s, "{main}");
	}
}

static bool same_obj(zval *v, zend_object *o)
{
	ZVAL_DEREF(v);
	return Z_TYPE_P(v) == IS_OBJECT && Z_OBJ_P(v) == o;
}

/* Where object o sits among the internal call's arguments: "N", "N[]", "this", or NULL. */
static void relation_of(smart_str *s, zend_execute_data *call, zend_object *o)
{
	uint32_t n = ZEND_CALL_NUM_ARGS(call);
	for (uint32_t i = 0; i < n; i++) {
		zval *a = ZEND_CALL_ARG(call, i + 1);
		if (same_obj(a, o)) { smart_str_append_long(s, i); return; }
	}
	for (uint32_t i = 0; i < n; i++) {
		zval *a = ZEND_CALL_ARG(call, i + 1);
		ZVAL_DEREF(a);
		if (Z_TYPE_P(a) == IS_ARRAY) {
			uint32_t seen = 0; zval *e;
			ZEND_HASH_FOREACH_VAL(Z_ARRVAL_P(a), e) {
				if (++seen > 256) break;
				if (same_obj(e, o)) { smart_str_append_long(s, i); smart_str_appends(s, "[]"); return; }
			} ZEND_HASH_FOREACH_END();
		}
	}
	if (Z_TYPE(call->This) == IS_OBJECT && Z_OBJ(call->This) == o) { smart_str_appends(s, "this"); return; }
	smart_str_appends(s, "-");
}

static void record(zend_execute_data *call, zend_execute_data *user)
{
	zend_function *uf = user->func;
	smart_str key = {0};
	append_name(&key, call->func);
	smart_str_appendc(&key, '\t');
	/* user side: method name, the class of $this, and its kind */
	if (uf->common.fn_flags & ZEND_ACC_CLOSURE) {
		smart_str_appends(&key, "{closure}");
	} else if (uf->common.function_name) {
		smart_str_append(&key, uf->common.function_name);
	} else {
		smart_str_appends(&key, "{main}");
	}
	smart_str_appendc(&key, '\t');
	if (Z_TYPE(user->This) == IS_OBJECT) {
		smart_str_append(&key, Z_OBJCE(user->This)->name);
	} else if (uf->common.scope) {
		smart_str_append(&key, uf->common.scope->name);
	} else {
		smart_str_appends(&key, "-");
	}
	smart_str_appendc(&key, '\t');
	/* the user object's place among the internal call's arguments */
	if (Z_TYPE(user->This) == IS_OBJECT && !(uf->common.fn_flags & ZEND_ACC_CLOSURE)) {
		relation_of(&key, call, Z_OBJ(user->This));
	} else {
		smart_str_appends(&key, "-");
	}
	smart_str_appendc(&key, '\t');
	/* a closure that is itself an argument (a callback) */
	if (uf->common.fn_flags & ZEND_ACC_CLOSURE) {
		relation_of(&key, call, ZEND_CLOSURE_OBJECT(uf));
	} else {
		smart_str_appends(&key, "-");
	}
	smart_str_appendc(&key, '\t');
	/* the user function's first argument, when it is an object of the internal call */
	if (ZEND_CALL_NUM_ARGS(user) > 0) {
		zval *a0 = ZEND_CALL_ARG(user, 1);
		zval *d = a0; ZVAL_DEREF(d);
		if (Z_TYPE_P(d) == IS_OBJECT) { relation_of(&key, call, Z_OBJ_P(d)); } else { smart_str_appends(&key, "-"); }
	} else {
		smart_str_appends(&key, "-");
	}
	smart_str_0(&key);
	if (!counts_ready) { zend_hash_init(&counts, 64, NULL, NULL, 1); counts_ready = true; }
	zval *c = zend_hash_str_find(&counts, ZSTR_VAL(key.s), ZSTR_LEN(key.s));
	if (c) { Z_LVAL_P(c)++; } else { zval one; ZVAL_LONG(&one, 1); zend_hash_str_add(&counts, ZSTR_VAL(key.s), ZSTR_LEN(key.s), &one); }
	smart_str_free(&key);
}

static void on_begin(zend_execute_data *ex)
{
	bool internal = ex->func->type == ZEND_INTERNAL_FUNCTION;
	if (!internal && depth > 0 && stack[depth - 1].internal) {
		record(stack[depth - 1].ex, ex);
	}
	if (depth < MAX_DEPTH) { stack[depth].ex = ex; stack[depth].internal = internal; }
	depth++;
}

static void on_end(zend_execute_data *ex, zval *rv)
{
	(void) rv;
	int i = (depth < MAX_DEPTH ? depth : MAX_DEPTH) - 1;
	while (i >= 0 && stack[i].ex != ex) i--;
	if (i >= 0) depth = i; else if (depth > 0 && depth > MAX_DEPTH) depth--;
}

static zend_observer_fcall_handlers on_init(zend_execute_data *ex)
{
	(void) ex;
	return (zend_observer_fcall_handlers){on_begin, on_end};
}

/* Calls every internal function through a real frame: with zend_execute_internal set, the
 * compiler emits no frameless calls, and ZEND_COMPILE_NO_BUILTINS stops strlen() and the
 * other special builtins from becoming opcodes. */
static void pass_through(zend_execute_data *ex, zval *rv)
{
	execute_internal(ex, rv);
}

PHP_MINIT_FUNCTION(cattrace)
{
	REGISTER_INI_ENTRIES();
	zend_observer_fcall_register(on_init);
	zend_execute_internal = pass_through;
	return SUCCESS;
}

PHP_RINIT_FUNCTION(cattrace)
{
	CG(compiler_options) |= ZEND_COMPILE_NO_BUILTINS;
	return SUCCESS;
}

PHP_RSHUTDOWN_FUNCTION(cattrace)
{
	const char *p = SG(request_info).path_translated;
	if (p) { strncpy(script, p, sizeof(script) - 1); }
	return SUCCESS;
}

PHP_MSHUTDOWN_FUNCTION(cattrace)
{
	const char *out = INI_STR("cattrace.output");
	if (counts_ready && out && *out) {
		smart_str buf = {0};
		zend_string *k; zval *v;
		ZEND_HASH_FOREACH_STR_KEY_VAL(&counts, k, v) {
			smart_str_append(&buf, k);
			smart_str_appendc(&buf, '\t');
			smart_str_append_long(&buf, Z_LVAL_P(v));
			smart_str_appendc(&buf, '\t');
			smart_str_appends(&buf, script);
			smart_str_appendc(&buf, '\n');
		} ZEND_HASH_FOREACH_END();
		smart_str_0(&buf);
		if (buf.s) {
			int fd = open(out, O_WRONLY | O_APPEND | O_CREAT, 0644);
			if (fd >= 0) { ssize_t w = write(fd, ZSTR_VAL(buf.s), ZSTR_LEN(buf.s)); (void) w; close(fd); }
		}
		smart_str_free(&buf);
	}
	if (counts_ready) { zend_hash_destroy(&counts); counts_ready = false; }
	UNREGISTER_INI_ENTRIES();
	return SUCCESS;
}

zend_module_entry cattrace_module_entry = {
	STANDARD_MODULE_HEADER, "cattrace", NULL,
	PHP_MINIT(cattrace), PHP_MSHUTDOWN(cattrace), PHP_RINIT(cattrace), PHP_RSHUTDOWN(cattrace), NULL,
	"0.1", STANDARD_MODULE_PROPERTIES
};

#ifdef COMPILE_DL_CATTRACE
ZEND_GET_MODULE(cattrace)
#endif
