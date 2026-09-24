<?php
            class Made { public function __construct(int $x) {} }
            function f(): void {
                $factory = new Made(...);
            }
        