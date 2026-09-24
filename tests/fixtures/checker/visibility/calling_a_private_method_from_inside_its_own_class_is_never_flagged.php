<?php
            class Foo {
                private function secret(): int { return 1; }
                public function reveal(): int {
                    return $this->secret();
                }
            }
        