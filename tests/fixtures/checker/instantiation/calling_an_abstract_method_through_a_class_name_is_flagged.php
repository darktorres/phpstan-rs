<?php
            abstract class AbsBase {
                abstract public function work(): void;
                abstract public static function build(): void;
            }
            class Impl extends AbsBase {
                public function work(): void { parent::work(); }
                public static function build(): void { parent::build(); static::build(); }
            }
        