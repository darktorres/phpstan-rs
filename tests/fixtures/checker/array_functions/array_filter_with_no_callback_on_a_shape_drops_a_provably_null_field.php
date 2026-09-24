<?php
            class GroupId {}
            /** @param list<GroupId> $ids */
            function sink(array $ids): void {}
            function f(GroupId $sourceId, ?GroupId $copyId): void {
                sink(array_filter([$sourceId, $copyId]));
            }
        