<?php
            function f(string|array $either, ?string $maybe): void {
                $either[] = 'x';
                $maybe[] = 'y';
            }
        