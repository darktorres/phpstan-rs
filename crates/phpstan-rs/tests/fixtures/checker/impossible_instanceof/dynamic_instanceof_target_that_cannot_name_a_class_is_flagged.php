<?php
            function f(object $o, int $i, string $s): void {
                if ($o instanceof $i) { echo 1; }
                if ($o instanceof $s) { echo 2; }
            }
        