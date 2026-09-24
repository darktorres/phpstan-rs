<?php
            class Widget {
                private int $one;
                private int $two;
                private function helper(): int { return 1; }
                public function magic(string $name): int {
                    return $this->{$name};
                }
            }
        