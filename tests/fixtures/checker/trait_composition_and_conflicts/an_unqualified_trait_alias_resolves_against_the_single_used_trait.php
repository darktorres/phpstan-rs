<?php
            trait X { public function greet(): string { return "x"; } }
            class Y {
                use X {
                    greet as greetAlias;
                }
            }
            function test(Y $y): void { echo $y->greet(); echo $y->greetAlias(); }
        