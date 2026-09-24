<?php
            function f(array $items): void {
                foreach ($items as $item) {
                    while (true) {
                        if ($item) {
                            continue 2;
                        }
                    }
                }
            }
        