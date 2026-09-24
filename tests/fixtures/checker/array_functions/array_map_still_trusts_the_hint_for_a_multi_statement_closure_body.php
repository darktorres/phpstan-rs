<?php
            /** @param list<string> $xs */
            function needsListOfStrings(array $xs): void {}
            /** @return list<int> */
            function ids(): array { return [1, 2, 3]; }
            $mapped = array_map(static function (int $x): string {
                if ($x < 0) {
                    throw new \RuntimeException('negative');
                }
                return (string) $x;
            }, ids());
            needsListOfStrings($mapped);
        