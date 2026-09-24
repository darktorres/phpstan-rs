<?php
            class Base { public function foo($x): void {} }
            class Child extends Base { public function foo(&$x): void {} }
        