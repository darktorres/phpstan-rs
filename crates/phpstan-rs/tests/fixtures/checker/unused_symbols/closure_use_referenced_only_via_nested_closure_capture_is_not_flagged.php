<?php
            function make(int $base) {
                return function () use ($base) {
                    return function () use ($base) {
                        return $base;
                    };
                };
            }
        