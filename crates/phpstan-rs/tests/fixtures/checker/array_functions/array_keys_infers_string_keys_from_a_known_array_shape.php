<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            function f() {
                $insert = ['id' => 1, 'galleries_url' => 'x'];
                needsStrings(array_keys($insert));
            }
        