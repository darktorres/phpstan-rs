<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $items): void {
                $collected = [];
                foreach ($items as $item) {
                    $collected[] = $item;
                }
                if (count($collected) > 0) {
                    sink($collected);
                }
            }
        