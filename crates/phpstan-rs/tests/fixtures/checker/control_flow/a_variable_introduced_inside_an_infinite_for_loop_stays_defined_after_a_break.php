<?php
            function f(array $items): void {
                for (;;) {
                    $found = null;
                    foreach ($items as $item) {
                        $found = $item;
                        break;
                    }
                    break;
                }
                echo $found;
            }
        