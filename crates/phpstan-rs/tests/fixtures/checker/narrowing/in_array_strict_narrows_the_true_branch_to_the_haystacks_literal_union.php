<?php
            /**
             * @param array<array-key, mixed> $post
             */
            function fromArray(array $post): string {
                $status_raw = $post['status'] ?? null;
                $status = in_array($status_raw, ['public', 'private'], true) ? $status_raw : '';
                return $status;
            }
        