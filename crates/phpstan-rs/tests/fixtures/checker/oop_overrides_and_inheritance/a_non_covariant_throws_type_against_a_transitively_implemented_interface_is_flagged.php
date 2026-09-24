<?php
            interface Fooer {
                /** @throws \LogicException */
                public function foo(): void;
            }
            class Widget implements Fooer {
                /** @throws \RuntimeException */
                public function foo(): void {}
            }
        