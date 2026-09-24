<?php
        function f(array $items): void {
            $n = 0;
            foreach ($items as $item) {
                if ($item) {
                    $n++;
                }
            }
            if ($n > 0) {
                echo 1;
            }
        }
        