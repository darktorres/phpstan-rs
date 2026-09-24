<?php
            class Base {
                /** @throws \Exception */
                public function foo(): void {}
            }
            class Child extends Base {
                /** @throws \RuntimeException */
                public function foo(): void {}
            }
        