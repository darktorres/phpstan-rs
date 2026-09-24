<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function f() {
                $list = ['x', 'y', 'z'];
                needsInts(array_keys($list));
            }
        