<?php
            class RealClass {}
            /** @phpstan-assert RealClass $x */
            function assertRealClass($x): void {}
        