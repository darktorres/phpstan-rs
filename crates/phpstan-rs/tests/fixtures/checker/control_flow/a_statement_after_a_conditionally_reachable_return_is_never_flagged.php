<?php
            function f(bool $cond): void {
                if ($cond) {
                    return;
                }
                echo "reachable";
            }
        