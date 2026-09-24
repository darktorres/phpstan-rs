<?php
            /** @throws void */
            function f(bool $cond): void {
                if ($cond) {
                    throw new \RuntimeException('nested but still f()\'s own body');
                }
            }
        