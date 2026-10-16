/**
 * @name Builtin reachability to user-code mechanisms
 * @description For every builtin function and method, the engine functions that dispatch to user
 *              code it can reach through CodeQL's call graph (static calls plus its function-pointer
 *              dispatch), without passing through user code itself.
 * @kind table
 * @id php/builtin-mechanism-reach
 */

import cpp
private import semmle.code.cpp.ir.dataflow.internal.DataFlowDispatch as Dispatch
private import semmle.code.cpp.ir.dataflow.internal.DataFlowPrivate as DFP

/** Entering user code: paths stop here, since user code can then call anything. */
predicate isUserEntry(Function f) {
  f.getName() = ["zend_call_function", "execute_ex", "zend_execute", "zend_execute_ex", "zend_execute_scripts"]
}

predicate isMechanism(Function f, string mechanism) {
  f.getName() = "zend_std_call_getter" and mechanism = "__get"
  or
  f.getName() = "zend_std_call_setter" and mechanism = "__set"
  or
  f.getName() = "zend_std_call_issetter" and mechanism = "__isset"
  or
  f.getName() = "zend_std_call_unsetter" and mechanism = "__unset"
  or
  f.getName() = "zend_std_cast_object_tostring" and mechanism = "__toString"
  or
  f.getName() = "zend_std_get_debug_info" and mechanism = "__debugInfo"
  or
  f.getName() = "zend_objects_destroy_object" and mechanism = "__destruct"
  or
  f.getName() = "zend_lazy_object_init" and mechanism = "lazy-initializer"
  or
  f.getName().matches("zend\\_user\\_it\\_%") and mechanism = "iterator"
  or
  f.getName() = ["zend_user_serialize", "zend_user_unserialize"] and mechanism = "Serializable"
  or
  f.getName().matches("user\\_wrapper\\_%") and mechanism = "wrapper:" + f.getName().suffix(13)
  or
  f.getName().matches("php\\_userstreamop\\_%") and mechanism = "stream:" + f.getName().suffix(17)
  or
  f.getName() = "zend_call_function" and mechanism = "any-user-code"
}

predicate edge(Function caller, Function callee) {
  not isUserEntry(caller) and
  exists(DFP::DataFlowCall call |
    call.getEnclosingCallable().getUnderlyingCallable() = caller and
    callee = Dispatch::viableCallable(call).getUnderlyingCallable()
  )
}

predicate isRoot(Function f) {
  f.getName().matches("zif\\_%") or f.getName().matches("zim\\_%")
}

predicate reaches(Function root, Function f) {
  isRoot(root) and edge(root, f)
  or
  exists(Function mid | reaches(root, mid) and edge(mid, f))
}

from Function root, Function sink, string mechanism
where reaches(root, sink) and isMechanism(sink, mechanism)
select root.getName() as builtin, mechanism, sink.getName() as via
