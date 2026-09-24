<?php
            trait T {}
            /** @phpstan-assert T $x */
            function f($x): void {}
        