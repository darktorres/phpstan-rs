<?php
            class Base {
                /** @throws \LogicException */
                public function foo(): void {}
            }
            class Child extends Base {
                /** @throws \RuntimeException */
                public function foo(): void {}
            }
        