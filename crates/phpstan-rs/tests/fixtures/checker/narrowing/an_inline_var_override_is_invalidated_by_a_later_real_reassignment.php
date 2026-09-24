<?php
            class FakeFileInfo9921 {}
            function needsInt(int $n): void {}
            function f($it): void {
                $file = $it;
                /** @var FakeFileInfo9921 $file */
                if ($it) {
                }
                $file = 5;
                needsInt($file);
            }
        