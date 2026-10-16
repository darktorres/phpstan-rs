/**
 * @name Indirect call resolution
 * @description How many calls through a function pointer CodeQL's dispatch resolves, and to how many targets.
 * @kind table
 * @id php/indirect-call-resolution
 */

import cpp
private import semmle.code.cpp.ir.dataflow.internal.DataFlowDispatch as Dispatch
private import semmle.code.cpp.ir.dataflow.internal.DataFlowPrivate as DFP

from DFP::DataFlowCall call, int targets
where
  not exists(call.getStaticCallTarget()) and
  targets = count(Dispatch::viableCallable(call))
select call.getEnclosingCallable().getUnderlyingCallable().(Function).getName() as caller,
  call.getLocation().getFile().getRelativePath() + ":" + call.getLocation().getStartLine() as site, targets
