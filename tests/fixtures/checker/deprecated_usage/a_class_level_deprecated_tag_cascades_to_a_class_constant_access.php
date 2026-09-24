<?php
            /** @deprecated */
            class OldFoo {
                const X = 1;
            }
            function f(): int {
                return OldFoo::X;
            }
        