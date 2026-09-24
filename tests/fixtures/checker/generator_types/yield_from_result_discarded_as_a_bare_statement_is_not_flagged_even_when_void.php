<?php
            /** @return \Generator<int, mixed, mixed, void> */
            function inner(): \Generator {
                yield 1;
            }
            /** @return \Generator<int, mixed> */
            function f(): \Generator {
                yield from inner();
            }
        