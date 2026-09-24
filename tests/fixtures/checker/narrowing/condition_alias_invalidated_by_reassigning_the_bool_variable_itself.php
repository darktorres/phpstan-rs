<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function otherBool1(): bool { return false; }
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                $ok = otherBool1();
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        