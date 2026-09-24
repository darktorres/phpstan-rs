<?php
            function byRef(&$r) {}
            function byVal($v) {}
            function f(array $a, array $items, string $s): void {
                byRef($a[]);
                preg_match('/x/', $s, $a[]);
                byVal($a[]);
                $a[] = 1;
                $a[][] = 2;
                $a[]['k'] = 3;
                foreach ($items as $a[]) {}
                [$a[], $a[]] = [1, 2];
                $r = &$a[];
                $a[] ??= 5;
                $a[] .= 'x';
                echo $a[];
            }
        