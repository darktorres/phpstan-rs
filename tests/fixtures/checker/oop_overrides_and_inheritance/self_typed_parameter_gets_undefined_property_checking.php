<?php
            class Point {
                public int $id = 0;
                public function equals(self $other): bool {
                    return $this->id === $other->missingProp;
                }
            }
        