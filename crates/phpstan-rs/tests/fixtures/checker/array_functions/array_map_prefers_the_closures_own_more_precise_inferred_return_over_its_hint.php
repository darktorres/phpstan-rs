<?php
            /** @param non-empty-string[] $xs */
            function needsNonEmptyStrings(array $xs): void {}
            $mapped = array_map(static function (int $x): string {
                return "not-empty";
            }, [1, 2, 3]);
            needsNonEmptyStrings($mapped);
        