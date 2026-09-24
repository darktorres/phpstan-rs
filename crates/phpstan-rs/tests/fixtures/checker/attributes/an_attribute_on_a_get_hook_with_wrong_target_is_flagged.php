<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class OnlyClass {}

            class C {
                public int $x {
                    #[OnlyClass]
                    get => 1;
                }
            }
        