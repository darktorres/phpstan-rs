<?php
            use function PHPStan\Testing\assertType;
            /** @param object{name: string, age: int} $x */
            function f($x): void {
                assertType('string', $x->name);
                assertType('int', $x->age);
            }
        