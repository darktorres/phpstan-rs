<?php
            class Base {
                /** @throws \LogicException */
                private function foo(): void {}
            }
            class Child extends Base {
                /** @throws \RuntimeException */
                private function foo(): void {}
            }
        