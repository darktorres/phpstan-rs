<?php
            /** @param array<int, array{rank: int}> $datas */
            function needsRankedList(array $datas): void {}
            /** @param int[] $items */
            function f(array $items): void {
                $counts = [];
                $datas = [];
                foreach ($items as $c) {
                    $rank = ++$counts[$c];
                    $datas[] = ['rank' => $rank];
                }
                needsRankedList($datas);
            }
        