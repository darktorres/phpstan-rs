<?php
            class Config {
                public static int $staticOne = 1;
            }
            function test(): int { return Config::$staticOne; }
        