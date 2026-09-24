<?php
            #[\Attribute(\Attribute::TARGET_FUNCTION)]
            class FunctionOnly {}

            function test(): void {
                $f = #[FunctionOnly] function () {};
            }
        