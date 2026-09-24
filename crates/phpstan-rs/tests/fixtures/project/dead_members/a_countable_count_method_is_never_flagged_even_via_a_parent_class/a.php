<?php abstract class Base implements Countable {}
class Foo extends Base { public function count(): int { return 0; } }