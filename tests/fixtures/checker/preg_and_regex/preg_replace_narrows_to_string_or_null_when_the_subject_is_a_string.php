<?php
            function f(string $subject): void {
                $result = preg_replace('/a/', 'b', $subject);
                if ($result !== null) {
                    strlen($result);
                }
            }
        