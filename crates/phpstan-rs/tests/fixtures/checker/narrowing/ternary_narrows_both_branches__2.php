<?php
            function f(?string $x): string {
                return $x === null ? $x : "default";
            }
        