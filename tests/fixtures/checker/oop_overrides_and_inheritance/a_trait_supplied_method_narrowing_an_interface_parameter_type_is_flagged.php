<?php
            interface Iface {
                public function foo(int $x): void;
            }
            trait T {
                public function foo(string $x): void {}
            }
            class Impl implements Iface {
                use T;
            }
        