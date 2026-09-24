<?php
            class Base6 { final public function locked(): void {} }
            class Mid6 extends Base6 {}
            class Leaf6 extends Mid6 { public function locked(): void {} }
        