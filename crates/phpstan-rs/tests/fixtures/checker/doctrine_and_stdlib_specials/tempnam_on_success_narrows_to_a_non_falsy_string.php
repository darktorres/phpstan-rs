<?php
            /** @param non-empty-string $path */
            function sink(string $path): void {}
            function f(): void {
                $jar = tempnam(sys_get_temp_dir(), 'x');
                if ($jar === false) {
                    throw new RuntimeException('x');
                }
                sink($jar);
            }
        