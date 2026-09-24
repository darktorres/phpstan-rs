<?php
            class Widget {
                public static function tryFromArray(array $data): ?self {
                    if (!isset($data['id'])) {
                        return null;
                    }
                    return new self();
                }
            }
        