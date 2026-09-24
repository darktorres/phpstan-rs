<?php
            function hideBlock(string $block_id): void {}
            /** @param array<int> $a
             *  @param array<int> $b
             */
            function f(array $a, array $b): void {
                $combined = $a + $b;
                foreach ($combined as $id => $v) {
                    hideBlock($id);
                }
            }
        