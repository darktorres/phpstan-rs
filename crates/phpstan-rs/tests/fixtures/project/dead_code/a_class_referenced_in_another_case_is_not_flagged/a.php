<?php
class Foo {}
function isFoo(object $o): bool { return $o instanceof foo; }
isFoo(new \stdClass());
