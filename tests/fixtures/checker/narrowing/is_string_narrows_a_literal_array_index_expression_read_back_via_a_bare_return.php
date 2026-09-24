<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return $info['code'];
                }
                return '';
            }
        