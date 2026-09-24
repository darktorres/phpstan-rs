<?php
            class Collection {
                public function count(): string { return "many"; }
            }
            /** @mixin Collection */
            class Query {
                public function count(): int { return 0; }
            }
            function needsInt(int $x): void {}
            function test(Query $q): void {
                needsInt($q->count());
            }
        