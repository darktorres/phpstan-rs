<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function f(Base1 $x): void {
                $x = $x instanceof Foo1;
                if ($x) {
                }
            }
        