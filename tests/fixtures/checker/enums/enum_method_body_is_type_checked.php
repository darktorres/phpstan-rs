<?php
            function needsInt(int $x): int { return $x; }
            enum Suit: string {
                case Hearts = 'H';
                public function bad(): string {
                    return needsInt("not an int");
                }
            }
        