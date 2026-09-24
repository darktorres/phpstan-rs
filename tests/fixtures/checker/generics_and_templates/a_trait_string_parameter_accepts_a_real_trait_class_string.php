<?php
            trait Helper3 {}
            /** @param trait-string $x */
            function f($x): void {}
            f(Helper3::class);
        