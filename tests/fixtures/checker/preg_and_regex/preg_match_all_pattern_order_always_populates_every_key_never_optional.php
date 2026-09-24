<?php
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches);
                // Group 2 is optional in a single match, but
                // PREG_PATTERN_ORDER always includes its own key
                // (possibly as an empty array) - reading it should never
                // be flagged as "no such key".
                echo count($matches[2]);
            }
        