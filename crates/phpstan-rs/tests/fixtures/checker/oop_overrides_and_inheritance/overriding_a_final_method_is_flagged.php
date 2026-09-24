<?php
            class Base4 { final public function locked(): int { return 1; } }
            class Sub4 extends Base4 { public function locked(): int { return 2; } }
        