<?php
            function needsNullableString(?string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                // Reading the field at all is fine (no "no such key") -
                // the field being present is still correctly preserved
                // through the Union.
                needsNullableString($matches[2]);
            }
        