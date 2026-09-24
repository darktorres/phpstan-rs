<?php
            function needsString(string $s): void {}
            /** @param array<int> $items */
            function f(array $items): void {
                $flipped = array_flip($items);
                foreach ($flipped as $k => $v) {
                    needsString($v);
                }
            }
        