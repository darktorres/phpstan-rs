<?php
            /** @return array<string, mixed>|false */
            function load(): array|false { return false; }
            function f(): void {
                $a = load();
                if ($a === false) {
                    echo 'x';
                    exit;
                }
                $b = load();
                if ($b === false) {
                    die;
                }
                foreach ($a as $v) {}
                foreach ($b as $v) {}
            }
        