<?php
            function memo(string $k): bool {
                static $cache = [];
                if (isset($cache[$k])) { return $cache[$k]; }
                return $cache[$k] = true;
            }
            class Holder {
                private array $p = [];
                public function fill(array $items): void {
                    $this->p = [];
                    foreach ($items as $k => $v) {
                        echo $this->p[$k] ?? 0;
                        if (isset($this->p[$k])) { echo $this->p[$k]; }
                        $this->p[$k] = $v;
                    }
                }
            }
            function job(): void {
                $job = ['id' => 1];
                $start = function () use (&$job): void { $job['process'] = 5; };
                $stop = function () use (&$job): void { $seen = $job['process']; };
            }
        