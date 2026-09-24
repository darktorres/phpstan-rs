<?php
            interface HasFoo {
                /** @return int|null */
                public function foo(): ?int;
            }
            final class Impl implements HasFoo {
                public function foo(): ?int {
                    return null;
                }
            }
        