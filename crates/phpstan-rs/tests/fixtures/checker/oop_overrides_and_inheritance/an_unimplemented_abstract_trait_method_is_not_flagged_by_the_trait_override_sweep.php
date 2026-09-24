<?php
            interface Iface {
                public function foo(int $x): void;
            }
            trait T {
                abstract public function foo(string $x): void;
            }
            class Impl implements Iface {
                use T;
            }
        