<?php
            function f(bool $x): void {
                if ($x) {
                    throw new \Exception();
                }
                return;
            }
        