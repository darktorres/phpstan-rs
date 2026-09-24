<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = [];
                assertType('null', key($a));
                assertType('null', array_key_first([]));
            }
        