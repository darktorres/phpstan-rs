<?php
            enum Status {
                case Active;
                public static function from($value): self { return self::Active; }
            }
        