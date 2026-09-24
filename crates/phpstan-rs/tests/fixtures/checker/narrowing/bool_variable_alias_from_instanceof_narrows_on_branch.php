<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function f(Base1 $x): void {
                $isFoo = $x instanceof Foo1;
                if ($isFoo) {
                    $x->fooOnlyMethod();
                }
            }
        