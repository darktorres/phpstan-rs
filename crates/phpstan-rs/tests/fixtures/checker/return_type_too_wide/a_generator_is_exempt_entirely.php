<?php
            /** @return \Generator<int, int|string> */
            function f(bool $b): \Generator {
                if ($b) {
                    yield 1;
                    return;
                }
                yield 2;
            }
        