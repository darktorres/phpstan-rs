<?php
            function f(int $n): void {
                for ($stop = false, $i = 0; !$stop && $i < $n; $i++) {
                    if ($i === 5) {
                        $stop = true;
                    }
                }
            }
        