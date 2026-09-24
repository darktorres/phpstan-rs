<?php
            /** @throws \RuntimeException */
            function f(): void {
                $g = function () {
                    throw new \RuntimeException('belongs to the closure, not f()');
                };
            }
        