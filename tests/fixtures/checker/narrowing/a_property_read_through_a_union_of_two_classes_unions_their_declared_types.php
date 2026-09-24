<?php
            use function PHPStan\Testing\assertType;
            class HasIntFlag { public int $flag = 1; }
            class HasStringFlag { public string $flag = "x"; }
            function f(HasIntFlag|HasStringFlag $v): void {
                assertType('int|string', $v->flag);
            }
        