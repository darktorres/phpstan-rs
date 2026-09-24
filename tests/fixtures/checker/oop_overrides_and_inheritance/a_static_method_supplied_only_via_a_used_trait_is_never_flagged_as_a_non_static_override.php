<?php
            class Base { public static function foo(): void {} }
            trait Provider { public static function foo(): void {} }
            class Child extends Base { use Provider; }
        