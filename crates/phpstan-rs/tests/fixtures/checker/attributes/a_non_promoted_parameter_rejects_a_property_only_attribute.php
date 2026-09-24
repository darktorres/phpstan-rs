<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                function f(#[PropOnly] int $x) {}
            }
        