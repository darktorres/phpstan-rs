<?php
            interface Comparator {
                /**
                 * @param array<string, mixed> $a
                 * @param array<string, mixed> $b
                 */
                public function compare(array $a, array $b): int;
            }
            function make(): Comparator {
                return new class implements Comparator {
                    public function compare(array $a, array $b): int {
                        return 0;
                    }
                };
            }
        