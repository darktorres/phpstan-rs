<?php
            trait Helper {
                public function realMethod($layerA, $layerB): int {
                    return $layerA + $layerB;
                }
            }
            /** @method int realMethod($target, $dependency) */
            class Host {
                use Helper;
            }
        