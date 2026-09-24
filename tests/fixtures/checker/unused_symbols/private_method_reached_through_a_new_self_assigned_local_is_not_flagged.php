<?php
            class Node {
                public function withSibling(): self {
                    $other = new self();
                    $other->helper();
                    return $other;
                }
                private function helper(): void {}
            }
        