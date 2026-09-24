<?php
            interface HasBreadcrumb13 {
                /** @return list<array{id: string, name: string}> */
                public function getCatBreadcrumb(string $x): array;
            }
            class FakeRenderer13 implements HasBreadcrumb13 {
                public function getCatBreadcrumb(string $x): array {
                    return [];
                }
            }
        