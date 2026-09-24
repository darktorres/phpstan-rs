<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                function __construct(#[PropOnly] public int $x) {}
            }
        