<?php
            /** @param non-empty-list<string> $x */
            function sink(array $x): void {}
            function f(string $a): void {
                $content = [];
                $content[] = $a;
                sink($content);
            }
        