<?php
            function needsString(string $s): void {}
            function f(string $s): void {
                $parts = explode("\x00", $s);
                $first = array_shift($parts);
                needsString($first);
            }
        