<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if ($info['code'] !== null) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        