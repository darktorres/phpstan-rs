<?php
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            /** @param list<array<string, mixed>> $rows */
            function f(array $rows): void {
                wantsList(array_column($rows, 'version'));
            }
        