<?php
            function test(): void {
                $g = fn(int ...$nums): int => count($nums);
            }
        