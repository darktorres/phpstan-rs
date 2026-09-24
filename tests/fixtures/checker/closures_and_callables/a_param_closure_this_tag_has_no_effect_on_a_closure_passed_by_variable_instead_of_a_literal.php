<?php
            class Secret6 { private int $value = 1; }
            /**
             * @param-closure-this Secret6 $cb
             */
            function withSecret6(Closure $cb) { return $cb(); }
            function test(): int {
                $cb = function () { return $this->value; };
                return withSecret6($cb);
            }
        