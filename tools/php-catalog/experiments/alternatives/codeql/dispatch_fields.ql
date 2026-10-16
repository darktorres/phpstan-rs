/**
 * @name Indirect call resolution, field based
 * @kind table
 * @id php/indirect-call-resolution-fields
 */

import cpp
import FieldResolution

from ExprCall c, int targets
where targets = count(indirectTarget(c))
select c.getEnclosingFunction().getName() as caller,
  c.getLocation().getFile().getRelativePath() + ":" + c.getLocation().getStartLine() as site, targets
