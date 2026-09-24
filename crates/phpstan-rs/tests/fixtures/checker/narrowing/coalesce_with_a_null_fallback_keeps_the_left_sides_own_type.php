<?php
            /**
             * @param array<array-key, mixed> $post
             * @param array<mixed> $default
             */
            function f(array $post, array $default): array {
                $selection_raw = $post['selection'] ?? null;
                return is_array($selection_raw) ? $selection_raw : $default;
            }
        