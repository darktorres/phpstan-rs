<?php
            function test(string $needle): void {
                preg_match('#' . preg_quote($needle, '.') . '#', "subject");
            }
        