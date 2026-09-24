<?php
            function test(): void {
                $f = function (int ...$nums): int { return count($nums); };
            }
        