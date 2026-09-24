<?php
            interface PreciseComparator {
                /**
                 * @param list<int> $a
                 */
                public function compare(array $a): int;
            }
            function make(): PreciseComparator {
                return new class implements PreciseComparator {
                    public function compare(array $a): int {
                        return 0;
                    }
                };
            }
        