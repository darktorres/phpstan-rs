<?php
            function needsInt(int $x): int { return $x; }
            function make() {
                return new class {
                    public function value(): int {
                        return needsInt(5);
                    }
                };
            }
        