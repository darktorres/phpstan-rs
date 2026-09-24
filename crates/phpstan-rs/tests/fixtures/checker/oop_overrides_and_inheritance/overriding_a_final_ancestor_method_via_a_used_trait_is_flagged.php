<?php
            class Base {
                final public function foo(): void {}
            }
            trait T {
                public function foo(): void {}
            }
            class Impl extends Base {
                use T;
            }
        