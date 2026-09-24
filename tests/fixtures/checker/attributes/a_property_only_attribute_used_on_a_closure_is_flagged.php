<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            function test(): void {
                $f = #[PropOnly] function () {};
            }
        