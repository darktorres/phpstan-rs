<?php
            /**
             * @template T of int|string
             */
            class PriorityQueue {
                /** @var array<T, mixed> */
                private array $heap = [];

                /** @param T $priority */
                public function insert($priority, $value): void {
                    $this->heap[$priority] = $value;
                }
            }
        