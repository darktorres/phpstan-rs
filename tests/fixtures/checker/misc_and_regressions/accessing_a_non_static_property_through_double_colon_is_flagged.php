<?php
            class Config {
                public int $instanceOnly = 0;
            }
            function test(): int { return Config::$instanceOnly; }
        