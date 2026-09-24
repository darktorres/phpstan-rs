<?php
            function takesByRef(&$y) { return $y; }
            function test(int $x) {
                $x |> 'takesByRef';
            }
        