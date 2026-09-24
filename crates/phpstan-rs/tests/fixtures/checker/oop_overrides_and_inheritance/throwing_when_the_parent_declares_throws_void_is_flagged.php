<?php
            class Base {
                /** @throws void */
                public function foo(): void {}
            }
            class Child extends Base {
                /** @throws \RuntimeException */
                public function foo(): void {}
            }
        