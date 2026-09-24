<?php
            class Mapper {
                public function mapAll(array $xs): array {
                    return array_map(fn ($x) => self::helper($x), $xs);
                }
                private static function helper(int $x): int { return $x * 2; }
            }
        