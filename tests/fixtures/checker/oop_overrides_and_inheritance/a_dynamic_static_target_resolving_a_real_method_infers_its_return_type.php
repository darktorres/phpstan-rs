<?php
            use function PHPStan\Testing\assertType;
            class Foo { public static function make(): string { return 'x'; } }
            /** @param Foo $obj */
            function f($obj) {
                assertType('string', $obj::make());
            }
        