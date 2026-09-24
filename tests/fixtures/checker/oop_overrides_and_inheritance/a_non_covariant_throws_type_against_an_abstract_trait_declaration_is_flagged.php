<?php
            trait FooTrait {
                /** @throws \LogicException */
                abstract public function foo(): void;
            }
            class Widget {
                use FooTrait;
                /** @throws \RuntimeException */
                public function foo(): void {}
            }
        