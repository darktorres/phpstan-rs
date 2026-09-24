<?php
            class Node {
                public int $x;
                public static function create(int $val): static {
                    $node = new static;
                    $node->x = $val;
                    return $node;
                }
            }
        