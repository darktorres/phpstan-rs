<?php
            function fromArray(?string $status_raw): string {
                $status = in_array($status_raw, ['public', 'private']) ? $status_raw : '';
                return $status;
            }
        