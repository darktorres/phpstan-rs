<?php
            function f9(string $pattern, string $subject, &$matches): int { $matches = []; return 1; }
            function test(string $s): void {
                f9(subject: $s, matches: $m, pattern: '/x/');
                echo $m;
            }
        