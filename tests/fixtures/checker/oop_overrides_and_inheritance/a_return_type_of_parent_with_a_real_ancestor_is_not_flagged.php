<?php
            class Animal {}
            class Dog extends Animal {
                public function f(): parent {
                    return $this;
                }
            }
        