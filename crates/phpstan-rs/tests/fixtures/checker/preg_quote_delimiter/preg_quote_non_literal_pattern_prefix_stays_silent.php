<?php
            function test(string $prefix, string $needle): void {
                preg_match($prefix . preg_quote($needle) . '/', "subject");
            }
        