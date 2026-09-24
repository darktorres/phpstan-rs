<?php
            /**
             * @property-read int $readOnlyTag
             * @property-write int $writeOnlyTag
             */
            class Magic {
                public function __get($n) { return 1; }
                public function __set($n, $v) {}
                public int $getOnly { get => 1; }
                public int $setOnly { set { echo $value; } }
            }
            function f(Magic $m): void {
                $m->readOnlyTag = 1;
                echo $m->writeOnlyTag;
                $m->getOnly = 2;
                echo $m->setOnly;
                echo $m->readOnlyTag;
                $m->writeOnlyTag = 3;
            }
        