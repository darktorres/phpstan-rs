<?php
            class Base {
                /** @throws \LogicException */
                public function foo(): void {}
            }
            class Child extends Base {
                /** @throws void */
                public function foo(): void {}
            }
        