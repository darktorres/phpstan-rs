<?php
            /** @return array{0: int, 1: string} */
            function pair(): array { return [1, "a"]; }
            function test(): string {
                [$num, $str] = pair();
                return $num;
            }
        