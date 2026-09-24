<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class ClassOnly {}

            class C {
                #[ClassOnly]
                const X = 1;
            }
        