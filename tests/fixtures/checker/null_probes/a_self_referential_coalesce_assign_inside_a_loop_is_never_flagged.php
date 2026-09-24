<?php
            function test(array $items): void {
                $err = null;
                foreach ($items as $item) {
                    if ($item) {
                        $err ??= $item;
                    }
                }
            }
        