<?php
            class Base {
                public function foo(string $x): void {}
            }
            trait T {
                abstract public function foo(int $x): void;
            }
            class Child extends Base {
                use T;
                public function foo(int $x): void {}
            }
        