<?php
            /** @return Closure(): array{status: int, body: string} */
            function make(): Closure {
                $f = static function (): array {
                    return ['status' => 200, 'body' => 'ok'];
                };
                return $f;
            }
            /** @return Closure(): int */
            function coerced(): Closure {
                return fn(): float => 1;
            }
        