<?php
            function f(): int {
                $x = null;
                do {
                    $x = 5;
                } while ($x === null);
                return $x;
            }
        