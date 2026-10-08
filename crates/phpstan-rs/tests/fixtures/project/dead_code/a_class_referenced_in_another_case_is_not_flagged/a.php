<?php
class Foo {}
function isFoo(object $o): bool { return $o instanceof foo; }
var_dump(isFoo(new \stdClass()));
