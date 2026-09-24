<?php
            /**
             * @param non-empty-string $path
             * @return array{dirname: string, basename: string, extension?: string, filename: string}
             */
            function f($path) {
                return pathinfo($path);
            }
        