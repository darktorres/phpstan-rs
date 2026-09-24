<?php
            #[\Attribute(\Attribute::TARGET_METHOD)]
            class MethodOnly {}

            #[MethodOnly]
            class C {}
        