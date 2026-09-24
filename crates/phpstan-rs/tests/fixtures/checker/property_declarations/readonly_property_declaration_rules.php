<?php
            class Ro {
                public readonly $untyped;
                public readonly int $defaulted = 1;
                public static readonly int $st;
                public function __construct(public readonly $promotedUntyped) {}
            }
            class ByDoc {
                /** @readonly */
                public int $withDefault = 1;
                /** @phpstan-readonly-allow-private-mutation */
                public int $allowed = 1;
                /** @readonly */
                public int $noDefault;
            }
        