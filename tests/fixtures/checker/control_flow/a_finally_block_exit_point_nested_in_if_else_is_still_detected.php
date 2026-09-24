<?php
            function f(bool $cond): int {
                try {
                    if ($cond) {
                        return 1;
                    } else {
                        return 2;
                    }
                } finally {
                    if ($cond) {
                        return 3;
                    } else {
                        return 4;
                    }
                }
            }
        