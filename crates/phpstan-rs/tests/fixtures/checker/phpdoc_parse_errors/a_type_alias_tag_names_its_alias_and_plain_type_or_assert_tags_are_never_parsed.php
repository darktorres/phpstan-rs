<?php
            /**
             * @phpstan-type Row array<int
             * @type Whatever array<int
             */
            class C {}
            /** @assert (1, 2) == 3 */
            function f() {}
        