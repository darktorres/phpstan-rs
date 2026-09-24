<?php
            /** @param string|array<string> $rawTags */
            function f($rawTags): int {
                if (! is_array($rawTags)) {
                    $rawTags = explode(',', $rawTags);
                }
                return count($rawTags);
            }
        