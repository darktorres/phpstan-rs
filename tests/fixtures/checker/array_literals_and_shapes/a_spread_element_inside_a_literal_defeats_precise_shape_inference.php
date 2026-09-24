<?php
            /** @param int[] $nums */
            function needsInts(array $nums): void {}
            function test(array $other): void {
                needsInts([1, ...$other, "not an int"]);
            }
        