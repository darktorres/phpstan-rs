<?php
            class Config {
                private const LIMIT = 10;
                public function make(): object {
                    return new class(self::LIMIT) {
                        public function __construct(public int $limit) {}
                    };
                }
            }
        