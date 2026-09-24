<?php
            interface Marker {}
            trait Checks {
                public function isMarker(): bool {
                    if ($this instanceof Marker) { return true; }
                    return false;
                }
            }
            class Marked implements Marker { use Checks; }
            class Unmarked { use Checks; }
        