<?php
            class Base {
                public function foo(string $x): void {}
            }
            interface Iface {
                public function foo(int $x): void;
            }
            class Child extends Base implements Iface {
                public function foo(int $x): void {}
            }
        