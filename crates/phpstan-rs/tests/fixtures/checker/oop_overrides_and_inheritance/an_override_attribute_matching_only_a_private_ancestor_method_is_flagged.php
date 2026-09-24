<?php
            class Base {
                private function foo(): void {}
            }
            class Child extends Base {
                #[\Override]
                public function foo(): void {}
            }
        