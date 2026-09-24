<?php
            class Node5 {}
            class SpecificNode5 extends Node5 {}
            interface Rule5 { public function refactor(Node5 $node): ?Node5; }
            class MyRector5 implements Rule5 {
                /** @param SpecificNode5 $node */
                public function refactor(SpecificNode5 $node): ?Node5 { return $node; }
            }
        