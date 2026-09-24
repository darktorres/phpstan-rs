<?php
            function test(string $needle): void {
                preg_replace('/' . preg_quote($needle) . '/', "x", "subject");
            }
        