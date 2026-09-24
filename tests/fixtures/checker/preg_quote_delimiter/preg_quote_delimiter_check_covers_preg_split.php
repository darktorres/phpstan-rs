<?php
            function test(string $needle): void {
                preg_split('/' . preg_quote($needle) . '/', "subject");
            }
        