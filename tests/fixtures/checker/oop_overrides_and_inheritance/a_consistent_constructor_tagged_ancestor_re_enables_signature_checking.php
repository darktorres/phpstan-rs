<?php
            /** @phpstan-consistent-constructor */
            class Base12 { public function __construct(string $a) {} }
            class Sub12 extends Base12 { public function __construct(int $a, int $b) {} }
        