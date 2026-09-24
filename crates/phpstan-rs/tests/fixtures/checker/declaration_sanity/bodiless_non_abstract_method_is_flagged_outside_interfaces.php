<?php
            class NoBody {
                public function run();
                public function fine() {}
            }
            abstract class AbsNoBody {
                abstract public function ok();
                public function missing();
            }
            interface Contract {
                public function run();
            }
            trait NoBodyTrait {
                public function run();
            }
        