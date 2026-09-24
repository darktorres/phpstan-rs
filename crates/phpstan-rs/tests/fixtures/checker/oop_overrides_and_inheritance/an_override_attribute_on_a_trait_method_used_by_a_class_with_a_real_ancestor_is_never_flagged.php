<?php
            trait T {
                #[\Override]
                public function foo(): void {}
            }
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                use T;
            }
        