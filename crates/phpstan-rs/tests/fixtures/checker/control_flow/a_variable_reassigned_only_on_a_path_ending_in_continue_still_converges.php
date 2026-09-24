<?php
            class Diff {}
            function needsString(string $x): void {}
            function f(bool $cond): array {
                $diffs = [];
                $diff  = null;
                while ($cond) {
                    if ($cond) {
                        if ($diff !== null) {
                            needsString($diff);
                            $diffs[] = $diff;
                        }
                        $diff = new Diff();
                        continue;
                    }
                }
                return $diffs;
            }
        