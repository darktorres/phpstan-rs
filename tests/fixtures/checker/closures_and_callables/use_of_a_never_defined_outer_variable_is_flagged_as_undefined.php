<?php
            function test(): int {
                $fn = function () use ($typo) {
                    return $typo;
                };
                return $fn();
            }
        