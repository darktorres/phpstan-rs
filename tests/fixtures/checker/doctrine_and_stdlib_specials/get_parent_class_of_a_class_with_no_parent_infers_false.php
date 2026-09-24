<?php
            class NoParent {
                public static function whoseParent(): string|false {
                    return get_parent_class(new self());
                }
            }
        