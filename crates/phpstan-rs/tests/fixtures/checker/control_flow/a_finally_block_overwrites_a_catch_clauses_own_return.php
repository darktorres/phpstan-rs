<?php
            function f(): int {
                try {
                    return 1;
                } catch (\Exception $e) {
                    return 2;
                } finally {
                    return 3;
                }
            }
        