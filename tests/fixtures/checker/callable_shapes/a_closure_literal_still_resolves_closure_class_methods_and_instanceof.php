<?php
            use function PHPStan\Testing\assertType;
            class C {}
            function takesClosure(Closure $c): void {}
            function takesCallable(callable $c): void {}
            function test(): void {
                $f = function (): int { return 1; };
                $bound = $f->bindTo(new C());
                takesClosure($f);
                takesCallable($f);
                if ($f instanceof Closure) {
                    takesClosure($f);
                }
            }
        