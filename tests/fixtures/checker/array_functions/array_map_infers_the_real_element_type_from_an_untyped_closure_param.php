<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(function ($x) { return $x * 2; }, [1, 2, 3]);
            needsStrings($mapped);
        