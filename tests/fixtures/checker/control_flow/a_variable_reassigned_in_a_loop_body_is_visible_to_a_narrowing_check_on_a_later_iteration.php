<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            function f() {
                $x = null;
                while (rand(0, 1)) {
                    if ($x !== null) {
                        $x->totallyMadeUp();
                    }
                    $x = new Foo();
                }
            }
        