<?php
            class Secret3 { private int $value = 42; }
            function test(Secret3 $s): int {
                $getter = Closure::bind(function () { return $this->value; }, $s, Secret3::class);
                return $getter();
            }
        