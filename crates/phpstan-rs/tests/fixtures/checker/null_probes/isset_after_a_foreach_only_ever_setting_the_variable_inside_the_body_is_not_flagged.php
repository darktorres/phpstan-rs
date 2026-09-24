<?php
            /** @param string[] $items */
            function test(array $items): void {
                foreach ($items as $item) {
                    $found = $item;
                }
                if (! isset($found)) {
                    return;
                }
                echo $found;
            }
        