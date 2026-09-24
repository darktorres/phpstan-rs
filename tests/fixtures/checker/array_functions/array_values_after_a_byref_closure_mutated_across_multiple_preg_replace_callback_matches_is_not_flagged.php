<?php
            function f(string $pattern, string $subject, array $value): array {
                $values = [];
                preg_replace_callback($pattern, static function (array $matches) use (&$values, $value): string {
                    if ($matches[1] ?? null) {
                        $values = [...$values, ...$value];
                    } else {
                        $values[] = $matches[0];
                    }

                    return '?';
                }, $subject);

                return array_values($values);
            }
        