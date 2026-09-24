<?php
            /** @param array<int>|null $tags */
            function f($tags): int {
                if (is_countable($tags) and count($tags) > 1) {
                    return count($tags);
                }
                return 0;
            }
        