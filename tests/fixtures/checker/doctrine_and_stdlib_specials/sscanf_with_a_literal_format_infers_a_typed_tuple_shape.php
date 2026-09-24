<?php
            /**
             * @return array{0: int|null, 1: string|null}|null
             */
            function f(string $line) {
                return sscanf($line, '%d %s');
            }
        