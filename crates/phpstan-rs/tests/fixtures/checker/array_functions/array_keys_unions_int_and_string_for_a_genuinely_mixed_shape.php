<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function f() {
                $mixed = [0 => 'a', 'name' => 'b'];
                needsInts(array_keys($mixed));
            }
        