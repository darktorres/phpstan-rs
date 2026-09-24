<?php
            /** @deprecated */
            function oldFunc(): void {}
            function test(): void {
                $f = oldFunc(...);
            }
        