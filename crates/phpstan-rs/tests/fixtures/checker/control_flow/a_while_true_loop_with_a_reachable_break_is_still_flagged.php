<?php
            function f(bool $c): string {
                while (true) {
                    if ($c) {
                        return 'x';
                    }
                    break;
                }
            }
        