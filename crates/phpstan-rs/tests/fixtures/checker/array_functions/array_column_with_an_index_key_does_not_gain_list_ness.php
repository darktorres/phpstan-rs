<?php
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            /** @param list<array{id: string, name: string}> $rows */
            function f(array $rows): void {
                wantsList(array_column($rows, 'name', 'id'));
            }
        