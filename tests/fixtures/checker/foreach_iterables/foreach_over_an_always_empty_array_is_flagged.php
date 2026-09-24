<?php
            function f(array $items): void {
                $empty = [];
                foreach ($empty as $x) { echo $x; }
                foreach ([] as $y) { echo $y; }
                foreach ($items as $z) { echo $z; }
                $grow = [];
                foreach ($items as $i) {
                    foreach ($grow as $g) { echo $g; }
                    $grow[] = $i;
                }
            }
        