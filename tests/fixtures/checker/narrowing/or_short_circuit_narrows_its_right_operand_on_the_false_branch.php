<?php
            function f(?string $x) {
                return $x === null || strlen($x) > 0;
            }
        