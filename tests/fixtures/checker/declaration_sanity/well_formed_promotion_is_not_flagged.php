<?php
            class OkHost {
                public function __construct(
                    private int $a,
                    readonly int $b,
                    private(set) string $c,
                    int ...$plain,
                ) {}
            }
            trait InitTrait {
                public function init(private int $x) {}
            }
            class AliasHost {
                use InitTrait { init as __construct; }
            }
        