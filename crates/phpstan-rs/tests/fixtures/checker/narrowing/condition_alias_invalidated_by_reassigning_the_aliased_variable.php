<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function other1(): Base1 { return new Base1(); }
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                $x = other1();
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        