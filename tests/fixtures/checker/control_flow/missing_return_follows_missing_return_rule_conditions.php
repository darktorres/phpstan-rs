<?php
            function nativeNullable(bool $b): ?int { if ($b) { return 1; } }
            /** @return int|null */
            function docNullable(bool $b) { if ($b) { return 1; } }
            function nativeMixed(bool $b): mixed { if ($b) { return 1; } }
            /** @return mixed */
            function docMixed(bool $b) { if ($b) { return 1; } }
            function voidOk(bool $b): void { if ($b) { return; } }
            $closure = function (bool $b): int { if ($b) { return 1; } };
            $untyped = function (bool $b) { if ($b) { return 1; } };
        