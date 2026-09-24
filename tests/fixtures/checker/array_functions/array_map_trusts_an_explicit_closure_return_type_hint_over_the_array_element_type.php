<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(fn(int $x): string => (string) $x, [1, 2, 3]);
            needsStrings($mapped);
        