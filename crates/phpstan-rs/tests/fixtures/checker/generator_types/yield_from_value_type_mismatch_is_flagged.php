<?php
            /** @return \Generator<int, string> */
            function inner(): \Generator {
                yield 'x';
            }
            /** @return \Generator<int, int> */
            function f(): \Generator {
                yield from inner();
            }
        