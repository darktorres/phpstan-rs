<?php
            /** @return \Generator<int, mixed, int> */
            function inner(): \Generator {
                yield 1;
            }
            /** @return \Generator<int, mixed, string> */
            function f(): \Generator {
                yield from inner();
            }
        