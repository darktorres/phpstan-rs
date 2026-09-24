<?php
            function test(int $x) {
                $x |> function (&$y) { return $y; };
            }
        