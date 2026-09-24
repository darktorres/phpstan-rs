<?php
            function f(): void {
                $end = false;
                while (!$end) {
                    if (rand(0, 1)) {
                        $end = true;
                    }
                }
            }
        