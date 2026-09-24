<?php
            function f(?string $x): string {
                if ($x) {
                    return $x;
                }
                return "default";
            }
        