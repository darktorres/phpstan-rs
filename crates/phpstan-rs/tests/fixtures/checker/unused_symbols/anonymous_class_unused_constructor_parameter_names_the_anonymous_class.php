<?php
            function make(): object {
                return new class(1) {
                    public function __construct(int $seed) {
                        echo 1;
                    }
                };
            }
        