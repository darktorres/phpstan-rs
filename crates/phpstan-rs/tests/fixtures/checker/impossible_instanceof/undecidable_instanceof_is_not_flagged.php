<?php
            interface Shape {}
            interface Named {}
            class Base {}
            class Child extends Base {}
            class Open {}
            function f(?Child $n, Base $b, Open $o, Named $named, mixed $m, object $obj): void {
                if ($n instanceof Child) { echo 1; }
                if ($b instanceof Child) { echo 2; }
                if ($o instanceof Shape) { echo 3; }
                if ($named instanceof Shape) { echo 4; }
                if ($m instanceof Shape) { echo 5; }
                if ($obj instanceof Shape) { echo 6; }
            }
        