<?php
            use function PHPStan\Testing\assertType;
            class Config { const LEVEL = 3; }
            function f(): mixed {
                $name = 'LEVEL';
                assertType('3', Config::{$name});
                return Config::{'MISSING'};
            }
        