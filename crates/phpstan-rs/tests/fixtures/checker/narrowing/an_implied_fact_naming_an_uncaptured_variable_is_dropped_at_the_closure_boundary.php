<?php
            function transactional(callable $cb): void {
                $cb();
            }
            function f(bool $flag): void {
                if ($flag) {
                    $other = "abc";
                }
                transactional(function () use ($flag): void {
                    if ($flag) {
                        echo $other;
                    }
                });
            }
        