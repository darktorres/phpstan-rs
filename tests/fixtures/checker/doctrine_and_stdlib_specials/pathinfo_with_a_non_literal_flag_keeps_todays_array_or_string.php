<?php
            function f(string $path, int $flags): array|string {
                return pathinfo($path, $flags);
            }
        