<?php
            #[\Attribute(\Attribute::TARGET_METHOD)]
            class MethodOnly {}

            enum Suit {
                #[MethodOnly]
                case Hearts;
            }
        