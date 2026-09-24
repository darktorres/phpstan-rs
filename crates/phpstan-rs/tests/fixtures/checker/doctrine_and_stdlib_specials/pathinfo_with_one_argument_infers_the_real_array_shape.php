<?php
            /**
             * @return array{dirname?: string, basename: string, extension?: string, filename: string}
             */
            function f(string $path) {
                return pathinfo($path);
            }
        