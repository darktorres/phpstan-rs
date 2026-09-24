<?php
            class Config {
                private const BASE = 1;
                private const DERIVED = self::BASE + 1;
                public function get(): int {
                    return self::DERIVED;
                }
            }
        