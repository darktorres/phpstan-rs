<?php
            function f(array $items): void {
                foreach ($items as $item) {
                    try {
                        continue;
                    } finally {
                        break;
                    }
                }
            }
        