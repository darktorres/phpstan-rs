<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a?: int}|false $x */
            function f($x, string $u): void {
                if (isset($x['a'])) {
                    assertType('array{a: int}', $x);
                }
                $parts = parse_url($u);
                if (isset($parts['port'])) {
                    echo $parts['port'];
                }
            }
        