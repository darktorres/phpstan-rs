<?php
            enum Status: int {
                case Active = 1;
                public static function tryFrom($value): ?self { return null; }
            }
        