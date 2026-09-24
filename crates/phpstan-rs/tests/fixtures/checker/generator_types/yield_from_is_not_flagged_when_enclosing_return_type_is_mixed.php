<?php
            /** @return \Generator<string, string> */
            function inner(): \Generator {
                yield 'a' => 'x';
            }
            function f(): \Generator {
                yield from inner();
            }
        