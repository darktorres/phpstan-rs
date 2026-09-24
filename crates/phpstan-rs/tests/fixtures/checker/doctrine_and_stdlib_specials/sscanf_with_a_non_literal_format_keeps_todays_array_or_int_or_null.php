<?php
            function f(string $line, string $format): array|int|null {
                return sscanf($line, $format);
            }
        