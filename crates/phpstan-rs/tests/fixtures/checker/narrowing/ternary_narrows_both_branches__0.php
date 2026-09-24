<?php
            function f(?string $x): string {
                return $x === null ? "default" : $x;
            }
        