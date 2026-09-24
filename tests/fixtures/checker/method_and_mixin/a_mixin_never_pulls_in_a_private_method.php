<?php
            class Collection {
                private function internalHelper(): int { return 0; }
            }
            /** @mixin Collection */
            class Query {}
            function test(Query $q): void {
                $q->internalHelper();
            }
        