<?php
            class Collection {
                public function count(): int { return 0; }
            }
            /** @mixin Collection */
            class Query {}
            function needsInt(int $x): void {}
            function test(Query $q): void {
                needsInt($q->count());
            }
        