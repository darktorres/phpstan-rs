<?php
            function fill(&$out): void { $out = 'x'; }
            function f(int $n): void {
                fill($n);
                /** @var string $s */
                $s = $n;
            }
        