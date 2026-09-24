<?php
            function needsString(string $s): void {}
            function handle(?int $x, ?string $y): void {
                $x = is_int($x) || is_string($y) ? $x : 0;
                needsString($x);
            }
        