<?php
            trait A { public function hello(): string { return "A"; } }
            trait B { public function hello(): string { return "B"; } }
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            function test(C $c): void { echo $c->hello(); echo $c->helloB(); }
        