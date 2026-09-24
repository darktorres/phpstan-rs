<?php
            function f(?string $a): void {
                if ($a !== null and strlen($a) > 0) {
                    echo 1;
                }
            }
        