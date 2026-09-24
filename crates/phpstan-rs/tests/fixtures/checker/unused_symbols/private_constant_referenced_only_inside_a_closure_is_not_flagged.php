<?php
            class Config {
                private const LIMIT = 10;
                public function makeChecker(): \Closure {
                    return function (int $x): bool {
                        return $x < self::LIMIT;
                    };
                }
            }
        