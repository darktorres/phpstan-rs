<?php
            #[\Attribute]
            class AnyTarget {}

            class C {
                #[AnyTarget]
                function f(): void {}

                #[AnyTarget]
                public int $x = 1;
            }
        