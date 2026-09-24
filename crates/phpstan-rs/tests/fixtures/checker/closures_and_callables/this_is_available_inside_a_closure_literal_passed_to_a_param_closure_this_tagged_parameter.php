<?php
            class Secret5 { public int $value = 7; }
            /**
             * @param-closure-this Secret5 $cb
             */
            function withSecret5(Closure $cb) { return $cb(); }
            function test(): int {
                return withSecret5(function () { return $this->value; });
            }
        