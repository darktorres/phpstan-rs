<?php
            class Backend {
                private bool $flag = false;
                public function compose(object $other): void {
                    if (! $other instanceof self) {
                        throw new \LogicException("nope");
                    }
                    if (! $other->flag) {
                        $other->flag = true;
                    }
                }
            }
        