<?php
            interface Iface {
                public function foo(): void;
            }
            class Impl implements Iface {
                #[\Override]
                public function foo(): void {}
            }
        