<?php
            function f(string $text) {
                if (preg_match('/(\d+)/', $text, $matches)) {
                    return $matches[1];
                }
                return null;
            }
        