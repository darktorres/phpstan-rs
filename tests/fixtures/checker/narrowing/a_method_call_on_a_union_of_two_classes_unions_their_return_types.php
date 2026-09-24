<?php
            use function PHPStan\Testing\assertType;
            class ReturnsInt { public function get(): int { return 1; } }
            class ReturnsString { public function get(): string { return "x"; } }
            function f(ReturnsInt|ReturnsString $v): void {
                assertType('int|string', $v->get());
            }
        