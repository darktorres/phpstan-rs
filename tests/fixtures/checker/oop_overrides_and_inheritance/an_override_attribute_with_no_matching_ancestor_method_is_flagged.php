<?php
            class Base {
                public function foo(): void {}
            }
            class Child extends Base {
                #[\Override]
                public function bar(): void {}
            }
        