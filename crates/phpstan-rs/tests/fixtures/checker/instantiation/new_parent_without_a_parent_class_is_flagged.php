<?php
            class Orphan {
                public function make(): object { return new parent(); }
            }
            class Base2 {}
            class Child2 extends Base2 {
                public function make(): object { return new parent(); }
            }
        