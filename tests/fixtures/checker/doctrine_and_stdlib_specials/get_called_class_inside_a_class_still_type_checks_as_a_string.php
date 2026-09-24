<?php
            class Base {
                public static function whoAmI(): string {
                    return get_called_class();
                }
            }
        