<?php
            /** @throws \RuntimeException */
            function f(callable $cb): void {
                $cb();
                echo 'never explicitly throws, but the dynamic call might';
            }
        