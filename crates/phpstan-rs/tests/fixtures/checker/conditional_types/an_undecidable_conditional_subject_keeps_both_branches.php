<?php
            use function PHPStan\Testing\assertType;
            interface Pkg {}
            class S {
                /** @param array<Pkg> $p
                 *  @return ($p is non-empty-array ? int : string) */
                public static function f(array $p) { return 1; }
                /** @param int|string $p
                 *  @return ($p is array ? int : string) */
                public static function g($p) { return ''; }
            }
            /** @return Pkg[] */ function find(): array { return []; }
            function t(): void {
                assertType('int|string', S::f(find()));
                assertType('string', S::g(1));
            }
        