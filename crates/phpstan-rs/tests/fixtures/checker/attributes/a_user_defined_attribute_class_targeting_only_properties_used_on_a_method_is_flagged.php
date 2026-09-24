<?php
            #[\Attribute(\Attribute::TARGET_PROPERTY)]
            class PropOnly {}

            class C {
                #[PropOnly]
                function f(): void {}
            }
        