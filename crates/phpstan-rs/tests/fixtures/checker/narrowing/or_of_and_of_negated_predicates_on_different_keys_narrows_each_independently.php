<?php
            function wantsThree(int|string $a, int|string $b, bool|int $c): void {}
            function handle(mixed $commentId, mixed $imageId, mixed $validated): void {
                if ((! is_int($commentId) && ! is_string($commentId))
                    || (! is_int($imageId) && ! is_string($imageId))
                    || (! is_bool($validated) && ! is_int($validated))) {
                    return;
                }
                wantsThree($commentId, $imageId, $validated);
            }
        