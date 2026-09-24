<?php
            function test(bool $cond): void {
                if ($cond) {
                    $x = 1;
                }
                if (isset($x)) {
                }
            }
        