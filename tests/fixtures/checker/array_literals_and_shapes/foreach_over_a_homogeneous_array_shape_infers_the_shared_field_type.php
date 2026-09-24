<?php
            function needsInt(int $n): int { return $n; }
            function test(): void {
                $versions = ['current' => 'a', 'latest' => 'b'];
                foreach ($versions as $key => $value) {
                    needsInt($value);
                }
            }
        