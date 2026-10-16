import cpp

/** A function whose address is the value `e` denotes, looking through casts. */
Function fnValue(Expr e) { result = e.getUnconverted().(FunctionAccess).getTarget() }

/** Functions stored in a field named `name`: aggregate initializers and assignments. */
Function storedInField(string name) {
  exists(ClassAggregateLiteral agg, Field f |
    f.getName() = name and result = fnValue(agg.getAFieldExpr(f))
  )
  or
  exists(AssignExpr a | a.getLValue().(FieldAccess).getTarget().getName() = name and result = fnValue(a.getRValue()))
}

/** Functions assigned to (or initializing) variable `v`. */
Function storedInVariable(Variable v) {
  result = fnValue(v.getInitializer().getExpr())
  or
  exists(AssignExpr a | a.getLValue().(VariableAccess).getTarget() = v and result = fnValue(a.getRValue()))
}

/** Functions passed as argument `i` to direct calls of `f`. */
Function passedTo(Function f, int i) {
  exists(FunctionCall c | c.getTarget() = f and result = fnValue(c.getArgument(i)))
}

Function indirectTarget(ExprCall c) {
  exists(Expr callee | callee = c.getExpr().getUnconverted() |
    result = storedInField(callee.(FieldAccess).getTarget().getName())
    or
    exists(Variable v | v = callee.(VariableAccess).getTarget() |
      not v instanceof Parameter and result = storedInVariable(v)
      or
      exists(Parameter p | p = v and result = passedTo(p.getFunction(), p.getIndex()))
    )
  )
}

