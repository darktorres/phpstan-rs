<?php
            #[\Attribute(\Attribute::TARGET_CLASS_CONSTANT)]
            class ConstOnly {}

            enum Suit {
                #[ConstOnly]
                case Hearts;
            }
        