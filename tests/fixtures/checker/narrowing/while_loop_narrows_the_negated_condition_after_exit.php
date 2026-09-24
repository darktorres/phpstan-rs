<?php
            function f(): int {
                $x = null;
                while ($x === null) {
                    $x = 5;
                }
                return $x;
            }
        