<?php
            function needsString(string $s): void {}
            /** @param array<mixed> $raw */
            function f(array $raw): void {
                $fields = [];
                foreach (array_keys($raw) as $fieldName) {
                    $fields[$fieldName] = 1;
                }
                foreach ($fields as $fieldName => $value) {
                    needsString($fieldName);
                }
            }
        