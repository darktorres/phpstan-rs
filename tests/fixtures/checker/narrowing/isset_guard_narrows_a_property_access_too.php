<?php
            class Box { public ?string $value = null; }
            function f(Box $box): int {
                if (isset($box->value)) {
                    return strlen($box->value);
                }
                return 0;
            }
        