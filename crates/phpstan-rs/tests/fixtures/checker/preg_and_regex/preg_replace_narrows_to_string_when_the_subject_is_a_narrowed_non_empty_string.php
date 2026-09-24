<?php
            function f(string $subject): string {
                if ($subject === '') {
                    return '';
                }
                $result = preg_replace('/a/', 'b', $subject);
                if ($result === null) {
                    return '';
                }
                return $result;
            }
        