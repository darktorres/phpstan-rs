<?php
class Foo { public function bar(): int { return 1; } }
class Magic { public function __call($n, $a) { return null; } }
class MagicP { public function __get($n) { return null; } }
class Plain {}
