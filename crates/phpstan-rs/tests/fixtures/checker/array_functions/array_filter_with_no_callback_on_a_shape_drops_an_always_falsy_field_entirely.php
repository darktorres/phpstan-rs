<?php
            class GroupId {}
            /** @param array{0: GroupId} $ids */
            function sink(array $ids): void {}
            function f(GroupId $sourceId): void {
                // Position 1 is a literal `null` - always falsy, so
                // array_filter's own result can never carry key 1 at
                // all, matching the exact `array{0: GroupId}` shape
                // `sink()` requires (not `array{0: GroupId, 1?: mixed}`).
                sink(array_filter([$sourceId, null]));
            }
        