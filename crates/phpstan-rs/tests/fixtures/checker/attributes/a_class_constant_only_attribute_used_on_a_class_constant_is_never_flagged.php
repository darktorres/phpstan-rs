<?php
            #[\Attribute(\Attribute::TARGET_CLASS_CONSTANT)]
            class ConstOnly {}

            class C {
                #[ConstOnly]
                const X = 1;
            }
        