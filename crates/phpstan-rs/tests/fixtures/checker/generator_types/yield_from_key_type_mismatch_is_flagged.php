<?php
            /** @return \Generator<string, mixed> */
            function inner(): \Generator {
                yield 1 => 'x';
            }
            /** @return \Generator<int, mixed> */
            function f(): \Generator {
                yield from inner();
            }
        