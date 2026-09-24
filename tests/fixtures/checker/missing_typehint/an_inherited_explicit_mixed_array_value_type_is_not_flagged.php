<?php
            interface I {
                /**
                 * @param array<string, mixed> $options
                 */
                public function f(array $options): void;
            }
            class C implements I {
                public function f(array $options): void {}
            }
        