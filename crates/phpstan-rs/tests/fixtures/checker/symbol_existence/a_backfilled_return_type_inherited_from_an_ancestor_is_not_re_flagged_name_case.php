<?php
            class Foo {}
            interface HasX {
                /** @return foo */
                public function getX();
            }
            class Impl implements HasX {
                public function getX() { return new Foo(); }
            }
        