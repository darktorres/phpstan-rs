<?php
            function plain(private int $a) {}
            class Host {
                public function method(public int $b) {}
                public function make() {
                    $f = function (protected int $c) {};
                    $g = fn (readonly int $d) => 1;
                    return new class {
                        public function inner(private int $e) {}
                    };
                }
            }
        