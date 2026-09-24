<?php
            function f(?string $a, ?string $b): void {
                if ($a === null or $b === null) {
                    return;
                }
                echo strlen($a) + strlen($b);
            }
        