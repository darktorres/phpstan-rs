<?php
            class NotFoundException {}
            function findNamespace(string $name): string { return $name; }
            function f(object $e, string $name): void {
                if ($e instanceof NotFoundException && $namespace = findNamespace($name)) {
                    echo $namespace;
                }
            }
        