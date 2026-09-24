<?php
            /** @phpstan-assert string[optional] $x */
            function a($x): void {}
            /** @phpstan-assert-if-true array $x */
            function b($x): bool { return true; }
        