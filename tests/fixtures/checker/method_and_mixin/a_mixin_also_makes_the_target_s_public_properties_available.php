<?php
            class Collection {
                public int $total = 0;
            }
            /** @mixin Collection */
            class Query {}
            function needsInt(int $x): void {}
            function test(Query $q): void {
                needsInt($q->total);
            }
        