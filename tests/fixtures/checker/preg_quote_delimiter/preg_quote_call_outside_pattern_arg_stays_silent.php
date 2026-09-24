<?php
            function test(string $needle, string $subject): void {
                preg_match('/foo/', preg_quote($needle) . $subject);
            }
        