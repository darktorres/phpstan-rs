<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        $result = "was-null";
                        break;
                    default:
                        try {
                            return "";
                        } catch (\Throwable $e) {
                            throw $e;
                        }
                }
                return $result;
            }
        