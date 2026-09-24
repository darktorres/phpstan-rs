<?php
            class Scripted {
                public function __construct(string $code, int $seed) {
                    eval($code);
                }
            }
        