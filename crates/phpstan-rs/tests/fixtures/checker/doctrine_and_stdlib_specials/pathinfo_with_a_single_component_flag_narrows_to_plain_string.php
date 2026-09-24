<?php
            function f(string $path): string {
                return pathinfo($path, PATHINFO_EXTENSION);
            }
        