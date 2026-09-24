<?php
            interface Iface {
                public function foo(): void;
            }
            trait T {
                protected function foo(): void {}
            }
            class Impl implements Iface {
                use T;
            }
        