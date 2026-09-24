<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function alwaysMixed(): mixed {
                return 1;
            }
            $mapped = array_map(static function (int $x): string {
                return alwaysMixed();
            }, [1, 2, 3]);
            needsStrings($mapped);
            needsInts($mapped);
        