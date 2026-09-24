<?php
            function f(bool $cond): int {
                if ($cond) {
                    goto skip;
                }
                return 1;
                skip:
                return 2;
            }
        