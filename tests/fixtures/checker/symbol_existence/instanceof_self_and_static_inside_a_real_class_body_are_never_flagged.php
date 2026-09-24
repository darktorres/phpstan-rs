<?php
            class Node {
                public function isSameKind($other): bool {
                    return $other instanceof self && $other instanceof static;
                }
            }
        