<?php
            function f(): void { self::go(); parent::go(); }
            class Lonely {
                public function m(): void { parent::go(); }
            }
        