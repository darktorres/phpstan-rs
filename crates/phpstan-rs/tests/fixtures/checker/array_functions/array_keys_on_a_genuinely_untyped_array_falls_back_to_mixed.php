<?php
            function needsStrings(string ...$xs): void {}
            function f(array $anything): void {
                needsStrings(...array_keys($anything));
            }
        