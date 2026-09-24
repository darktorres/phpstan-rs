<?php
            class Money {}
            class Whatever {}
            /**
             * @template T
             */
            class ParentGeneric {
                /** @return T */
                public function parentValue() { return null; }
            }
            /**
             * @template T
             * @extends ParentGeneric<Whatever>
             */
            class Child extends ParentGeneric {
                /** @param T $value */
                public function __construct(private $value) {}
                /** @return T */
                public function ownValue() { return $this->value; }
            }
            /**
             * @param Child<Money> $c
             * @return string
             */
            function f($c): string {
                return $c->ownValue();
            }
        