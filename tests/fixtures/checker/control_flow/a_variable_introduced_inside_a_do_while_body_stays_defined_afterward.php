<?php
            function f(array $items): void {
                do {
                    $changed = false;
                    foreach ($items as $item) {
                        if ($item) {
                            $changed = true;
                        }
                    }
                } while ($changed);
                echo $changed;
            }
        