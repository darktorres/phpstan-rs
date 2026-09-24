<?php
            function fetch(): string|false { return 'x'; }
            function f(): string {
                if (is_string($result = fetch())) {
                    return $result;
                }
                return '';
            }
        