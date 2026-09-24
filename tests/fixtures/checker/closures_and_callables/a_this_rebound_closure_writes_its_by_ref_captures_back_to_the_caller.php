<?php
            use function PHPStan\Testing\assertType;
            function beforeEach(?Closure $c = null): void {}
            function afterEach(?Closure $c = null): void {}
            $orig = null;
            beforeEach(function () use (&$orig): void {
                $orig = getenv('A');
            });
            afterEach(function () use (&$orig): void {
                assertType('null|string|false', $orig);
                putenv($orig === false || $orig === null ? 'A' : 'A=' . $orig);
            });
            class Foo { private int $x = 1; }
            $n = null;
            $f = Closure::bind(function () use (&$n): void { $n = $this->x; }, new Foo(), Foo::class);
            assertType('null|int', $n);
        