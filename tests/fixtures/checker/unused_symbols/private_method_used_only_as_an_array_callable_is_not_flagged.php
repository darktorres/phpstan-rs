<?php
            class Sorter {
                public function sort(array $items): array {
                    usort($items, [$this, 'compare']);
                    return $items;
                }
                private function compare(int $a, int $b): int { return $a <=> $b; }
            }
        