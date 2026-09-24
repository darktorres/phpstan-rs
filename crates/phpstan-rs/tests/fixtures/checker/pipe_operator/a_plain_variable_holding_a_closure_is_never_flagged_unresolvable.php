<?php
            function test(int $x, callable $cb) {
                $x |> $cb;
            }
        