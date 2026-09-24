<?php
            /**
             * @property int $magic
             */
            class Base {}
            class Sub extends Base {}
            function f(Sub $s): int { return $s->magic; }
        