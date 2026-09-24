<?php
            /** @param string[] $items */
            function test(array $items, bool $cond): void {
                $file_path = null;
                if ($cond) {
                    foreach ($items as $item) {
                        $file_path = $item;
                    }
                }
                if (! isset($file_path)) {
                    return;
                }
                echo $file_path;
            }
        