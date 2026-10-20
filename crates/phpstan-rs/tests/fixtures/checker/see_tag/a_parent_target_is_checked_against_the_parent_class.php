<?php
            class Base {
                public function run(): void {}
            }
            class Real extends Base {
                /** @see parent::run() */
                public function bar(): void {}
            }
