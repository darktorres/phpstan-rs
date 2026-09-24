<?php
            class Secret4 { private int $value = 99; }
            function test(Secret4 $s): int {
                $bound = (function () { return $this->value; })->bindTo($s, Secret4::class);
                return $bound();
            }
        