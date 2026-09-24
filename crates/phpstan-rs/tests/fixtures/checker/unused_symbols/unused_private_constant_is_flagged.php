<?php
            class Config {
                private const UNUSED = 1;
                private const USED = 2;
                public function get(): int {
                    return self::USED;
                }
            }
        