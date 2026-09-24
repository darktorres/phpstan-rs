<?php
            /** @return array<int, string> */
            function group(): array { return []; }
            function fromLangArray(int $index): string {
                $data = group();
                $val = $data[$index] ?? null;
                return $val ?? '';
            }
        